use super::*;
use crate::cap_pool::{CapPoolCalculator, CapacityInfo};
use shared::bee_msg::misc::RefreshCapacityPools;
use shared::bee_msg::target::{
    RefreshTargetStates, SetTargetConsistencyStates, SetTargetConsistencyStatesResp,
};
use shared::bee_msg::OpsErr;
use std::time::Duration;

impl CapacityInfo for &pm::get_targets_response::Target {
    fn free_space(&self) -> u64 {
        self.free_space_bytes.unwrap()
    }

    fn free_inodes(&self) -> u64 {
        self.free_inodes.unwrap()
    }
}

/// Delivers the list of targets
pub(crate) async fn get(
    ctx: Context,
    _req: pm::GetTargetsRequest,
) -> Result<pm::GetTargetsResponse> {
    let node_offline_timeout = ctx.info.user_config.node_offline_timeout;
    let pre_shutdown = ctx.run_state.pre_shutdown();

    let targets_q = sql!(
        "SELECT t.target_uid, t.alias, t.target_id, t.node_type,
            n.node_uid, n.alias, n.node_id,
            p.pool_uid, p.alias, p.pool_id,
            t.consistency, (UNIXEPOCH('now') - UNIXEPOCH(last_contact)),
            t.free_space, t.free_inodes, t.total_space, t.total_inodes,
            g.s_target_id
        FROM targets_ext AS t
        INNER JOIN nodes_ext AS n USING(node_uid)
        LEFT JOIN pools_ext AS p USING(node_type, pool_id)
        LEFT JOIN buddy_groups AS g ON g.s_target_id = t.target_id AND g.node_type = t.node_type"
    );

    let targets_f = move |row: &rusqlite::Row| {
        let node_type = NodeType::from_row(row, 3)?.into_proto_i32();

        Ok(pm::get_targets_response::Target {
            id: Some(pb::EntityIdSet {
                uid: row.get(0)?,
                legacy_id: Some(pb::LegacyId {
                    num_id: row.get(2)?,
                    node_type,
                }),
                alias: row.get(1)?,
            }),
            node_type,
            node: Some(pb::EntityIdSet {
                uid: row.get(4)?,
                legacy_id: Some(pb::LegacyId {
                    num_id: row.get(6)?,
                    node_type,
                }),
                alias: row.get(5)?,
            }),
            storage_pool: if let Some(uid) = row.get::<_, Option<Uid>>(7)? {
                Some(pb::EntityIdSet {
                    uid: Some(uid),
                    legacy_id: Some(pb::LegacyId {
                        num_id: row.get(9)?,
                        node_type,
                    }),
                    alias: row.get(8)?,
                })
            } else {
                None
            },

            reachability_state: if !pre_shutdown || row.get::<_, Option<TargetId>>(16)?.is_some() {
                calc_reachability_state(Duration::from_secs(row.get(11)?), node_offline_timeout)
                    .into()
            } else {
                pb::ReachabilityState::Poffline.into()
            },
            consistency_state: TargetConsistencyState::from_row(row, 10)?.into_proto_i32(),
            last_contact_s: row.get(11)?,
            free_space_bytes: row.get(12)?,
            free_inodes: row.get(13)?,
            cap_pool: pb::CapacityPool::Unspecified.into(),
            total_space_bytes: row.get(14)?,
            total_inodes: row.get(15)?,
        })
    };

    let pools_q = sql!("SELECT pool_uid FROM storage_pools");

    let (mut targets, pools): (Vec<pm::get_targets_response::Target>, Vec<Uid>) = ctx
        .db
        .read_tx(move |tx| {
            Ok((
                tx.query_map_collect(targets_q, [], targets_f)?,
                tx.query_map_collect(pools_q, [], |row| row.get(0))?,
            ))
        })
        .await
        .status_code(Code::Internal)?;

    let cap_pool_meta_calc = CapPoolCalculator::new(
        ctx.info.user_config.cap_pool_meta_limits.clone(),
        ctx.info.user_config.cap_pool_dynamic_meta_limits.as_ref(),
        targets
            .iter()
            .filter(|t| t.node_type() == pb::NodeType::Meta),
    )?;

    for t in targets
        .iter_mut()
        .filter(|t| t.node_type() == pb::NodeType::Meta)
    {
        if t.free_space_bytes.is_some() && t.free_inodes.is_some() {
            t.cap_pool = pb::CapacityPool::from(
                cap_pool_meta_calc.cap_pool(t.free_space_bytes.unwrap(), t.free_inodes.unwrap()),
            )
            .into();
        }
    }

    for sp_uid in pools {
        let cap_pool_storage_calc = CapPoolCalculator::new(
            ctx.info.user_config.cap_pool_storage_limits.clone(),
            ctx.info
                .user_config
                .cap_pool_dynamic_storage_limits
                .as_ref(),
            targets.iter().filter(|t| {
                t.storage_pool
                    .as_ref()
                    .is_some_and(|e| e.uid == Some(sp_uid))
            }),
        )?;

        for t in targets.iter_mut().filter(|t| {
            t.storage_pool
                .as_ref()
                .is_some_and(|e| e.uid == Some(sp_uid))
        }) {
            if t.free_space_bytes.is_some() && t.free_inodes.is_some() {
                t.cap_pool = pb::CapacityPool::from(
                    cap_pool_storage_calc
                        .cap_pool(t.free_space_bytes.unwrap(), t.free_inodes.unwrap()),
                )
                .into();
            }
        }
    }

    Ok(pm::GetTargetsResponse { targets })
}

/// Deletes a target
pub(crate) async fn delete(
    ctx: Context,
    req: pm::DeleteTargetRequest,
) -> Result<pm::DeleteTargetResponse> {
    fail_on_pre_shutdown(&ctx)?;

    let target: EntityId = required_field(req.target)?.try_into()?;
    let execute: bool = required_field(req.execute)?;

    let target = ctx
        .db
        .conn(move |conn| {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

            let target = target.resolve(&tx, EntityType::Target)?;

            if target.node_type() != NodeType::Storage {
                bail!("Only storage targets can be deleted directly");
            }

            let assigned_groups: usize = tx.query_row_cached(
                sql!(
                    "SELECT COUNT(*) FROM buddy_groups_ext
                    WHERE p_target_uid = ?1 OR s_target_uid = ?1"
                ),
                [target.uid],
                |row| row.get(0),
            )?;

            if assigned_groups > 0 {
                bail!("Target {target} is part of a buddy group");
            }

            db::target::delete_storage(&tx, target.num_id().try_into()?)?;

            if execute {
                tx.commit()?;
            }
            Ok(target)
        })
        .await?;

    if execute {
        log::info!("Target deleted: {target}");

        notify_nodes(
            &ctx,
            &[NodeType::Meta],
            &RefreshCapacityPools { ack_id: "".into() },
        )
        .await;
    }

    let target = Some(target.into());

    log::warn!("{target:?}");

    Ok(pm::DeleteTargetResponse { target })
}

/// Calculate reachability state
pub(crate) fn calc_reachability_state(
    contact_age: Duration,
    timeout: Duration,
) -> pb::ReachabilityState {
    if contact_age > timeout {
        pb::ReachabilityState::Offline
    } else if contact_age > timeout / 2 {
        pb::ReachabilityState::Poffline
    } else {
        pb::ReachabilityState::Online
    }
}

/// Set consistency state for a target
pub(crate) async fn set_state(
    ctx: Context,
    req: pm::SetTargetStateRequest,
) -> Result<pm::SetTargetStateResponse> {
    fail_on_pre_shutdown(&ctx)?;

    let state: TargetConsistencyState = req.consistency_state().try_into()?;
    let target: EntityId = required_field(req.target)?.try_into()?;

    let (target, node_uid) = ctx
        .db
        .write_tx(move |tx| {
            let target = target.resolve(tx, EntityType::Target)?;

            let node: i64 = tx.query_row_cached(
                sql!("SELECT node_uid FROM targets_ext WHERE target_uid = ?1"),
                [target.uid],
                |row| row.get(0),
            )?;

            db::target::update_consistency_states(
                tx,
                [(target.num_id().try_into()?, state)],
                NodeTypeServer::try_from(target.node_type())?,
            )?;

            Ok((target, node))
        })
        .await?;

    let resp: SetTargetConsistencyStatesResp = ctx
        .conn
        .request(
            node_uid,
            &SetTargetConsistencyStates {
                node_type: target.node_type(),
                target_ids: vec![target.num_id().try_into().unwrap()],
                states: vec![state],
                ack_id: "".into(),
                set_online: 0,
            },
        )
        .await?;
    if resp.result != OpsErr::SUCCESS {
        bail!("Management successfully set the target state, but the target {target} failed to process it: {:?}", resp.result);
    }

    notify_nodes(
        &ctx,
        &[NodeType::Meta, NodeType::Storage, NodeType::Client],
        &RefreshTargetStates { ack_id: "".into() },
    )
    .await;

    Ok(pm::SetTargetStateResponse {})
}
