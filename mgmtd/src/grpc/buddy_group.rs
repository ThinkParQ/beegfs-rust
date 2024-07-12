use super::*;
use db::misc::MetaRoot;
use protobuf::{beegfs as pb, management as pm};
use shared::bee_msg::buddy_group::{
    RemoveBuddyGroup, RemoveBuddyGroupResp, SetMetadataMirroring, SetMetadataMirroringResp,
    SetMirrorBuddyGroup,
};
use shared::bee_msg::OpsErr;

/// Delivers the list of buddy groups
pub(crate) async fn get(
    ctx: &Context,
    _req: pm::GetBuddyGroupsRequest,
) -> Result<pm::GetBuddyGroupsResponse> {
    let buddy_groups = ctx
        .db
        .op(|tx| {
            Ok(tx.query_map_collect(
                sql!(
                    "SELECT group_uid, group_id, bg.alias, bg.node_type,
                        p_target_uid, p_t.target_id, p_t.alias,
                        s_target_uid, s_t.target_id, s_t.alias,
                        sp.pool_uid, bg.pool_id, e_sp.alias,
                        p_t.consistency, s_t.consistency
                    FROM all_buddy_groups_v AS bg
                    INNER JOIN all_targets_v AS p_t ON p_t.target_uid = p_target_uid
                    INNER JOIN all_targets_v AS s_t ON s_t.target_uid = s_target_uid
                    LEFT JOIN storage_pools AS sp ON sp.pool_id = bg.pool_id
                    LEFT JOIN entities AS e_sp ON e_sp.uid = sp.pool_uid"
                ),
                [],
                |row| {
                    let node_type = i32::from(pb::NodeType::from(NodeType::from_row(row, 3)?));
                    let p_con_state = i32::from(pb::ConsistencyState::from(
                        TargetConsistencyState::from_row(row, 13)?,
                    ));
                    let s_con_state = i32::from(pb::ConsistencyState::from(
                        TargetConsistencyState::from_row(row, 14)?,
                    ));

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
    ctx: &Context,
    req: pm::CreateBuddyGroupRequest,
) -> Result<pm::CreateBuddyGroupResponse> {
    let node_type: NodeTypeServer = req.node_type().try_into()?;
    let alias: Alias = required_field(req.alias)?.try_into()?;
    let num_id: BuddyGroupId = req.num_id.unwrap_or_default().try_into()?;
    let p_target: EntityId = required_field(req.primary_target)?.try_into()?;
    let s_target: EntityId = required_field(req.secondary_target)?.try_into()?;

    let (group, p_target, s_target) = ctx
        .db
        .op(move |tx| {
            let p_target = p_target.resolve(tx, EntityType::Target)?;
            let s_target = s_target.resolve(tx, EntityType::Target)?;

            let (group_uid, group_id) = db::buddy_group::insert(
                tx,
                num_id,
                &alias,
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
        ctx,
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
    ctx: &Context,
    req: pm::DeleteBuddyGroupRequest,
) -> Result<pm::DeleteBuddyGroupResponse> {
    let group: EntityId = required_field(req.group)?.try_into()?;
    let execute: bool = required_field(req.execute)?;

    // 1. Check deletion is allowed
    let (group, p_node_uid, s_node_uid) = ctx
        .db
        .op_with_conn(move |conn| {
            let tx = conn.transaction()?;

            let group = group.resolve(&tx, EntityType::BuddyGroup)?;

            if group.node_type() != &NodeType::Storage {
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
        .op_with_conn(move |conn| {
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
    ctx: &Context,
    _req: pm::MirrorRootInodeRequest,
) -> Result<pm::MirrorRootInodeResponse> {
    let meta_root = ctx
        .db
        .op(|tx| {
            let node_uid = match db::misc::get_meta_root(tx)? {
                MetaRoot::Normal(_, node_uid) => node_uid,
                MetaRoot::Mirrored(_) => bail!("Root inode is already mirrored"),
                MetaRoot::Unknown => bail!("Root inode unknown"),
            };

            let count = tx.query_row(
                sql!(
                    "SELECT COUNT(*) FROM root_inode AS ri
                        INNER JOIN meta_buddy_groups AS mg ON mg.p_target_id = ri.target_id"
                ),
                [],
                |row| row.get::<_, i64>(0),
            )?;

            if count < 1 {
                bail!("The meta target holding the root inode is not part of a buddy group.");
            }

            Ok(node_uid)
        })
        .await?;

    let resp: SetMetadataMirroringResp = ctx
        .conn
        .request(meta_root, &SetMetadataMirroring {})
        .await?;

    match resp.result {
        OpsErr::SUCCESS => ctx.db.op(db::misc::enable_metadata_mirroring).await?,
        _ => bail!("Root inode mirroring failed with Error {:?}", resp.result),
    }

    log::info!("Root inode has been mirrored");
    Ok(pm::MirrorRootInodeResponse {})
}
