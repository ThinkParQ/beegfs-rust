use super::*;
use shared::bee_msg::OpsErr;
use shared::bee_msg::buddy_group::{
    BuddyResyncJobState, GetMetaResyncStats, GetMetaResyncStatsResp, GetStorageResyncStats,
    GetStorageResyncStatsResp, SetLastBuddyCommOverride,
};
use shared::bee_msg::target::{RefreshTargetStates, SetTargetConsistencyStatesResp};
use std::time::{Duration, Instant};
use tokio::time::sleep;

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
