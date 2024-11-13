use super::*;
use db::misc::MetaRoot;
use protobuf::{beegfs as pb, management as pm};
use shared::bee_msg::buddy_group::{
    BuddyResyncJobState, GetMetaResyncStats, GetMetaResyncStatsResp, GetStorageResyncStats,
    GetStorageResyncStatsResp, RemoveBuddyGroup, RemoveBuddyGroupResp, SetLastBuddyCommOverride,
    SetMetadataMirroring, SetMetadataMirroringResp, SetMirrorBuddyGroup,
};
use shared::bee_msg::target::{RefreshTargetStates, SetTargetConsistencyStatesResp};
use shared::bee_msg::OpsErr;
use tokio::time::{sleep, Duration, Instant};

/// Delivers the list of buddy groups
pub(crate) async fn get(
    ctx: Context,
    _req: pm::GetBuddyGroupsRequest,
) -> Result<pm::GetBuddyGroupsResponse> {
    let buddy_groups = ctx
        .db
        .read_tx(|tx| {
            Ok(tx.query_map_collect(
                sql!(
                    "SELECT group_uid, group_id, bg.alias, bg.node_type,
                        p_target_uid, p_t.target_id, p_t.alias,
                        s_target_uid, s_t.target_id, s_t.alias,
                        p.pool_uid, bg.pool_id, p.alias,
                        p_t.consistency, s_t.consistency
                    FROM buddy_groups_ext AS bg
                    INNER JOIN targets_ext AS p_t ON p_t.target_uid = p_target_uid
                    INNER JOIN targets_ext AS s_t ON s_t.target_uid = s_target_uid
                    LEFT JOIN pools_ext AS p USING(node_type, pool_id)"
                ),
                [],
                |row| {
                    let node_type = NodeType::from_row(row, 3)?.into_proto_i32();
                    let p_con_state = TargetConsistencyState::from_row(row, 13)?.into_proto_i32();
                    let s_con_state = TargetConsistencyState::from_row(row, 14)?.into_proto_i32();

                    Ok(pm::get_buddy_groups_response::BuddyGroup {
                        id: Some(pb::EntityIdSet {
                            uid: row.get(0)?,
                            legacy_id: Some(pb::LegacyId {
                                num_id: row.get(1)?,
                                node_type,
                            }),
                            alias: row.get(2)?,
                        }),
                        node_type,
                        primary_target: Some(pb::EntityIdSet {
                            uid: row.get(4)?,
                            legacy_id: Some(pb::LegacyId {
                                num_id: row.get(5)?,
                                node_type,
                            }),
                            alias: row.get(6)?,
                        }),
                        secondary_target: Some(pb::EntityIdSet {
                            uid: row.get(7)?,
                            legacy_id: Some(pb::LegacyId {
                                num_id: row.get(8)?,
                                node_type,
                            }),
                            alias: row.get(9)?,
                        }),
                        storage_pool: if let Some(uid) = row.get::<_, Option<Uid>>(10)? {
                            Some(pb::EntityIdSet {
                                uid: Some(uid),
                                legacy_id: Some(pb::LegacyId {
                                    num_id: row.get(11)?,
                                    node_type,
                                }),
                                alias: row.get(12)?,
                            })
                        } else {
                            None
                        },
                        primary_consistency_state: p_con_state,
                        secondary_consistency_state: s_con_state,
                    })
                },
            )?)
        })
        .await?;

    Ok(pm::GetBuddyGroupsResponse { buddy_groups })
}

/// Creates a new buddy group
pub(crate) async fn create(
    ctx: Context,
    req: pm::CreateBuddyGroupRequest,
) -> Result<pm::CreateBuddyGroupResponse> {
    needs_license(&ctx, LicensedFeature::Mirroring)?;
    fail_on_pre_shutdown(&ctx)?;

    let node_type: NodeTypeServer = req.node_type().try_into()?;
    let alias: Alias = required_field(req.alias)?.try_into()?;
    let num_id: BuddyGroupId = req.num_id.unwrap_or_default().try_into()?;
    let p_target: EntityId = required_field(req.primary_target)?.try_into()?;
    let s_target: EntityId = required_field(req.secondary_target)?.try_into()?;

    let (group, p_target, s_target) = ctx
        .db
        .write_tx(move |tx| {
            let p_target = p_target.resolve(tx, EntityType::Target)?;
            let s_target = s_target.resolve(tx, EntityType::Target)?;

            let (group_uid, group_id) = db::buddy_group::insert(
                tx,
                num_id,
                Some(alias.clone()),
                node_type,
                p_target.num_id().try_into()?,
                s_target.num_id().try_into()?,
            )?;
            Ok((
                EntityIdSet {
                    uid: group_uid,
                    alias,
                    legacy_id: LegacyId {
                        node_type: node_type.into(),
                        num_id: group_id.into(),
                    },
                },
                p_target,
                s_target,
            ))
        })
        .await?;

    log::info!("Buddy group created: {group}");

    notify_nodes(
        &ctx,
        &[NodeType::Meta, NodeType::Storage, NodeType::Client],
        &SetMirrorBuddyGroup {
            ack_id: "".into(),
            node_type: node_type.into(),
            primary_target_id: p_target.num_id().try_into()?,
            secondary_target_id: s_target.num_id().try_into()?,
            group_id: group.num_id().try_into()?,
            allow_update: 0,
        },
    )
    .await;

    Ok(pm::CreateBuddyGroupResponse {
        group: Some(group.into()),
    })
}

/// Deletes a buddy group. This function is racy as it is a two step process, talking to other
/// nodes in between. Since it is rarely used, that's ok though.
pub(crate) async fn delete(
    ctx: Context,
    req: pm::DeleteBuddyGroupRequest,
) -> Result<pm::DeleteBuddyGroupResponse> {
    needs_license(&ctx, LicensedFeature::Mirroring)?;
    fail_on_pre_shutdown(&ctx)?;

    let group: EntityId = required_field(req.group)?.try_into()?;
    let execute: bool = required_field(req.execute)?;

    // 1. Check deletion is allowed
    let (group, p_node_uid, s_node_uid) = ctx
        .db
        .conn(move |conn| {
            let tx = conn.transaction()?;

            let group = group.resolve(&tx, EntityType::BuddyGroup)?;

            if group.node_type() != NodeType::Storage {
                bail!("Only storage buddy groups can be deleted");
            }

            let (p_node_uid, s_node_uid) =
                db::buddy_group::prepare_storage_deletion(&tx, group.num_id().try_into()?)?;

            if execute {
                tx.commit()?;
            }
            Ok((group, p_node_uid, s_node_uid))
        })
        .await?;

    // 2. Forward request to the groups nodes
    let group_id: BuddyGroupId = group.num_id().try_into()?;
    let remove_bee_msg = RemoveBuddyGroup {
        node_type: NodeType::Storage,
        group_id,
        check_only: if execute { 0 } else { 1 },
        force: 0,
    };

    let p_res: RemoveBuddyGroupResp = ctx.conn.request(p_node_uid, &remove_bee_msg).await?;
    let s_res: RemoveBuddyGroupResp = ctx.conn.request(s_node_uid, &remove_bee_msg).await?;

    if p_res.result != OpsErr::SUCCESS || s_res.result != OpsErr::SUCCESS {
        bail!(
            "Removing storage buddy group on primary and/or secondary storage node failed. \
Primary result: {:?}, Secondary result: {:?}",
            p_res.result,
            s_res.result
        );
    }

    // 3. If the deletion request succeeded, remove the group from the database
    ctx.db
        .conn(move |conn| {
            let tx = conn.transaction()?;

            db::buddy_group::delete_storage(&tx, group_id)?;

            if execute {
                tx.commit()?;
            }
            Ok(())
        })
        .await?;

    if execute {
        log::info!("Buddy group deleted: {}", group);
    }

    Ok(pm::DeleteBuddyGroupResponse {
        group: Some(group.into()),
    })
}

/// Enable metadata mirroring for the root directory
pub(crate) async fn mirror_root_inode(
    ctx: Context,
    _req: pm::MirrorRootInodeRequest,
) -> Result<pm::MirrorRootInodeResponse> {
    needs_license(&ctx, LicensedFeature::Mirroring)?;
    fail_on_pre_shutdown(&ctx)?;

    let meta_root = ctx
        .db
        .read_tx(|tx| {
            let node_uid = match db::misc::get_meta_root(tx)? {
                MetaRoot::Normal(_, node_uid) => node_uid,
                MetaRoot::Mirrored(_) => bail!("Root inode is already mirrored"),
                MetaRoot::Unknown => bail!("Root inode unknown"),
            };

            let count = tx.query_row(
                sql!(
                    "SELECT COUNT(*) FROM root_inode AS ri
                    INNER JOIN buddy_groups AS mg
                        ON mg.p_target_id = ri.target_id AND mg.node_type = ?1"
                ),
                [NodeType::Meta.sql_variant()],
                |row| row.get::<_, i64>(0),
            )?;

            if count < 1 {
                bail!("The meta target holding the root inode is not part of a buddy group.");
            }

            // Check that no clients are connected to prevent data corruption. Note that there is
            // still a small chance for a client being mounted again before the action is taken on
            // the root meta server below. In the end, it's the administrators responsibility to not
            // let any client mount during that process.
            let clients = tx.query_row(sql!("SELECT COUNT(*) FROM client_nodes"), [], |row| {
                row.get::<_, i64>(0)
            })?;

            if clients > 0 {
                bail!("This operation requires that all clients are disconnected/unmounted, but still has {clients} clients mounted.");
            }

            Ok(node_uid)
        })
        .await?;

    let resp: SetMetadataMirroringResp = ctx
        .conn
        .request(meta_root, &SetMetadataMirroring {})
        .await?;

    match resp.result {
        OpsErr::SUCCESS => ctx.db.write_tx(db::misc::enable_metadata_mirroring).await?,
        _ => bail!(
            "The root meta server failed to mirror the root inode: {:?}",
            resp.result
        ),
    }

    log::info!("Root inode has been mirrored");
    Ok(pm::MirrorRootInodeResponse {})
}

/// Starts a resync of a storage or metadata target from its buddy target
pub(crate) async fn start_resync(
    ctx: Context,
    req: pm::StartResyncRequest,
) -> Result<pm::StartResyncResponse> {
    needs_license(&ctx, LicensedFeature::Mirroring)?;
    fail_on_pre_shutdown(&ctx)?;

    let buddy_group: EntityId = required_field(req.buddy_group)?.try_into()?;
    let timestamp: i64 = required_field(req.timestamp)?;
    let restart: bool = required_field(req.restart)?;

    // For resync source is always primary target and destination is secondary target
    let (src_target_id, dest_target_id, src_node_uid, node_type, group) = ctx
        .db
        .read_tx(move |tx| {
            let group = buddy_group.resolve(tx, EntityType::BuddyGroup)?;
            let node_type: NodeTypeServer = group.node_type().try_into()?;

            let (src_target_id, dest_target_id, src_node_uid): (TargetId, TargetId, Uid) = tx
                .query_row_cached(
                    sql!(
                        "SELECT g.p_target_id, g.s_target_id, src_t.node_uid
                        FROM buddy_groups AS g
                        INNER JOIN targets_ext AS src_t
                            ON src_t.target_id = g.p_target_id AND src_t.node_type = g.node_type
                        WHERE group_uid = ?1"
                    ),
                    [group.uid],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )?;

            Ok((
                src_target_id,
                dest_target_id,
                src_node_uid,
                node_type,
                group,
            ))
        })
        .await?;

    // We handle meta and storage servers separately as storage servers allow restarts and metas
    // dont.
    match node_type {
        NodeTypeServer::Meta => {
            if timestamp > -1 {
                bail!(
                    "Metadata targets can only do full resync, timestamp and timespan is \
not supported."
                );
            }
            if restart {
                bail!("Resync cannot be restarted or aborted for metadata servers.");
            }

            let resp: GetMetaResyncStatsResp = ctx
                .conn
                .request(
                    src_node_uid,
                    &GetMetaResyncStats {
                        target_id: src_target_id,
                    },
                )
                .await?;

            if resp.state == BuddyResyncJobState::Running {
                bail!("Resync already running on buddy group {group}");
            }
        }
        NodeTypeServer::Storage => {
            if !restart {
                let resp: GetStorageResyncStatsResp = ctx
                    .conn
                    .request(
                        src_node_uid,
                        &GetStorageResyncStats {
                            target_id: src_target_id,
                        },
                    )
                    .await?;

                if resp.state == BuddyResyncJobState::Running {
                    bail!("Resync already running on buddy group {group}");
                }

                if timestamp > -1 {
                    override_last_buddy_comm(&ctx, src_node_uid, src_target_id, &group, timestamp)
                        .await?;
                }
            } else {
                if timestamp < 0 {
                    bail!("Resync for storage targets can only be restarted with timestamp.");
                }

                override_last_buddy_comm(&ctx, src_node_uid, src_target_id, &group, timestamp)
                    .await?;

                log::info!("Waiting for the already running resync operations to abort.");

                let timeout = tokio::time::Duration::from_secs(180);
                let start = Instant::now();

                // This sleep and poll loop is bad style, but the simplest way to do it for
                // now. A better solution would be to intercept the message from the server that
                // tells us the resync is finished, but that is a bit more complex and, with the
                // current system, still unreliable.
                loop {
                    let resp: GetStorageResyncStatsResp = ctx
                        .conn
                        .request(
                            src_node_uid,
                            &GetStorageResyncStats {
                                target_id: src_target_id,
                            },
                        )
                        .await?;

                    if resp.state != BuddyResyncJobState::Running {
                        break;
                    }

                    if start.elapsed() >= timeout {
                        bail!("Timeout. Unable to abort resync on buddy group {group}");
                    }

                    sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }

    // set destination target state as needs-resync in mgmtd database
    ctx.db
        .write_tx(move |tx| {
            db::target::update_consistency_states(
                tx,
                [(dest_target_id, TargetConsistencyState::NeedsResync)],
                node_type,
            )?;
            Ok(())
        })
        .await?;

    // This also triggers the source node to fetch the new needs resync state and start the resync
    // using the internode syncer loop. In case of overriding last buddy communication on storage
    // nodes, this means that there is a max 3s window where the communication timestamp can be
    // overwritten again before resync starts, effectively ignoring it. There is nothing we can do
    // about that without changing the storage server.
    //
    // Note that sending a SetTargetConsistencyStateMsg does have no effect on making this quicker,
    // so we omit it.
    notify_nodes(
        &ctx,
        &[NodeType::Meta, NodeType::Storage, NodeType::Client],
        &RefreshTargetStates { ack_id: "".into() },
    )
    .await;

    return Ok(pm::StartResyncResponse {});

    /// Override last buddy communication timestamp on source storage node
    /// Note that this might be overwritten again on the storage server between
    async fn override_last_buddy_comm(
        ctx: &Context,
        src_node_uid: Uid,
        src_target_id: TargetId,
        group: &EntityIdSet,
        timestamp: i64,
    ) -> Result<()> {
        let resp: SetTargetConsistencyStatesResp = ctx
            .conn
            .request(
                src_node_uid,
                &SetLastBuddyCommOverride {
                    target_id: src_target_id,
                    timestamp,
                    abort_resync: 0,
                },
            )
            .await?;

        if resp.result != OpsErr::SUCCESS {
            bail!(
                "Could not override last buddy communication timestamp on primary of buddy group {group}. \
Failed with resp {:?}",
                resp.result
            );
        }

        Ok(())
    }
}
