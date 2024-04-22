use super::*;
use crate::cap_pool::{CapPoolCalculator, CapacityInfo};
use crate::types::{CapacityPool, NodeTypeServer, TargetConsistencyState};
use pb::beegfs::beegfs as pb;
use std::time::Duration;

impl CapacityInfo for &pb::get_targets_response::Target {
    fn free_space(&self) -> u64 {
        self.free_space_bytes.unwrap()
    }

    fn free_inodes(&self) -> u64 {
        self.free_inodes.unwrap()
    }
}

pub(crate) async fn get(ctx: &Context, _req: GetTargetsRequest) -> Result<GetTargetsResponse> {
    let node_offline_timeout = ctx.info.user_config.node_offline_timeout;

    let targets_q = sql!(
        "SELECT t.target_uid, t.alias, t.target_id, t.node_type,
            n.node_uid, n.alias, n.node_id,
            sp.pool_uid, e_sp.alias, sp.pool_id,
            t.consistency, n.last_contact_s, t.free_space, t.free_inodes,
            t.total_space, t.total_inodes
        FROM all_targets_v AS t
        INNER JOIN all_nodes_v AS n USING(node_uid)
        LEFT JOIN storage_pools AS sp USING(pool_id)
        LEFT JOIN entities AS e_sp ON e_sp.uid = sp.pool_uid
        WHERE n.node_id IS NOT NULL"
    );

    let targets_f = move |row: &rusqlite::Row| {
        let node_type = match row.get(3)? {
            NodeTypeServer::Meta => pb::NodeType::Meta,
            NodeTypeServer::Storage => pb::NodeType::Storage,
        } as i32;

        Ok(pb::get_targets_response::Target {
            id: Some(pb::EntityIdSet {
                uid: row.get(0)?,
                legacy_id: Some(LegacyId {
                    num_id: row.get(2)?,
                    node_type,
                    entity_type: EntityType::Target as i32,
                }),
                alias: row.get(1)?,
            }),
            node_type,
            node: Some(pb::EntityIdSet {
                uid: row.get(4)?,
                legacy_id: Some(LegacyId {
                    num_id: row.get(6)?,
                    node_type,
                    entity_type: EntityType::Node as i32,
                }),
                alias: row.get(5)?,
            }),
            storage_pool: if let Some(uid) = row.get::<_, Option<EntityUID>>(7)? {
                Some(pb::EntityIdSet {
                    uid,
                    legacy_id: Some(LegacyId {
                        num_id: row.get(9)?,
                        node_type,
                        entity_type: EntityType::StoragePool as i32,
                    }),
                    alias: row.get(8)?,
                })
            } else {
                None
            },

            reachability_state: calc_reachability_state(
                Duration::from_secs(row.get(11)?),
                node_offline_timeout,
            ) as i32,
            consistency_state: match row.get(10)? {
                TargetConsistencyState::Good => target::ConsistencyState::Good,
                TargetConsistencyState::NeedsResync => target::ConsistencyState::NeedsResync,
                TargetConsistencyState::Bad => target::ConsistencyState::Bad,
            } as i32,
            last_contact_s: row.get(11)?,
            free_space_bytes: row.get(12)?,
            free_inodes: row.get(13)?,
            cap_pool: pb::CapacityPool::Unspecified as i32,
            total_space_bytes: row.get(14)?,
            total_inodes: row.get(15)?,
        })
    };

    let pools_q = sql!("SELECT pool_uid FROM storage_pools");

    let (mut targets, pools): (Vec<pb::get_targets_response::Target>, Vec<EntityUID>) = ctx
        .db
        .op(move |tx| {
            Ok((
                tx.query_map_collect(targets_q, [], targets_f)?,
                tx.query_map_collect(pools_q, [], |row| row.get(0))?,
            ))
        })
        .await
        .map_err(|e| Status::new(Code::Internal, error_chain!(e, "Getting targets failed")))?;

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
            t.cap_pool = match cap_pool_meta_calc
                .cap_pool(t.free_space_bytes.unwrap(), t.free_inodes.unwrap())
            {
                CapacityPool::Normal => pb::CapacityPool::Normal,
                CapacityPool::Low => pb::CapacityPool::Low,
                CapacityPool::Emergency => pb::CapacityPool::Emergency,
            } as i32;
        }
        println!("{:?}", t.cap_pool());
    }

    for sp_uid in pools {
        let cap_pool_storage_calc = CapPoolCalculator::new(
            ctx.info.user_config.cap_pool_storage_limits.clone(),
            ctx.info
                .user_config
                .cap_pool_dynamic_storage_limits
                .as_ref(),
            targets
                .iter()
                .filter(|t| t.storage_pool.as_ref().is_some_and(|e| e.uid == sp_uid)),
        )?;

        for t in targets
            .iter_mut()
            .filter(|t| t.storage_pool.as_ref().is_some_and(|e| e.uid == sp_uid))
        {
            if t.free_space_bytes.is_some() && t.free_inodes.is_some() {
                t.cap_pool = match cap_pool_storage_calc
                    .cap_pool(t.free_space_bytes.unwrap(), t.free_inodes.unwrap())
                {
                    CapacityPool::Normal => pb::CapacityPool::Normal,
                    CapacityPool::Low => pb::CapacityPool::Low,
                    CapacityPool::Emergency => pb::CapacityPool::Emergency,
                } as i32;
            }
        }
    }

    Ok(GetTargetsResponse { targets })
}

/// Calculate reachability state as known by old BeeGFS code.
pub(crate) fn calc_reachability_state(
    contact_age: Duration,
    timeout: Duration,
) -> target::ReachabilityState {
    if contact_age > timeout {
        target::ReachabilityState::Offline
    } else if contact_age > timeout / 2 {
        target::ReachabilityState::Poffline
    } else {
        target::ReachabilityState::Online
    }
}
