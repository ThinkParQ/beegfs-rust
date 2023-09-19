use super::*;
use shared::msg::types::{BuddyGroupCapacityPools, TargetCapacityPools};
use shared::types::{BuddyGroupID, NodeID, TargetID};

pub(super) async fn handle(
    _msg: msg::GetStoragePools,
    ctx: &Context,
    _req: &impl Request,
) -> msg::GetStoragePoolsResp {
    let pools = match async move {
        let config = &ctx.info.config;

        let (targets, pools, buddy_groups) = ctx
            .db
            .op(move |tx| {
                let pools = db::storage_pool::get_all(tx)?;
                let targets = db::cap_pool::for_storage_targets(
                    tx,
                    &config.cap_pool_storage_limits,
                    config.cap_pool_dynamic_storage_limits.as_ref(),
                )?;
                let buddy_groups = db::cap_pool::for_storage_buddy_groups(
                    tx,
                    &config.cap_pool_storage_limits,
                    config.cap_pool_dynamic_storage_limits.as_ref(),
                )?;

                Ok((targets, pools, buddy_groups))
            })
            .await?;

        // Build the data structures msg::GetStoragePool wants, per pool
        pools
            .into_iter()
            .map(|pool| {
                // IDs belonging to the three cap pools
                let mut target_cap_pools = [vec![], vec![], vec![]];
                let mut buddy_group_cap_pools = [vec![], vec![], vec![]];

                // NodeID -> Vec<TargetID> map for each cap pool
                let mut grouped_target_cap_pools = [
                    HashMap::<NodeID, Vec<TargetID>>::new(),
                    HashMap::new(),
                    HashMap::new(),
                ];

                // Target / buddy group info without cap pools
                let mut target_map: HashMap<TargetID, NodeID> = HashMap::new();
                let mut buddy_group_vec: Vec<BuddyGroupID> = vec![];

                // Only collect targets belonging to the current pool
                for target in targets.iter().filter(|t| t.pool_id == pool.pool_id) {
                    let cap_pool_i: usize = target.cap_pool.into();
                    let target_id: TargetID = target.entity_id;

                    target_map.insert(target_id, target.node_id);
                    target_cap_pools[cap_pool_i].push(target_id);

                    if let Some(node_group) =
                        grouped_target_cap_pools[cap_pool_i].get_mut(&target.node_id)
                    {
                        node_group.push(target_id);
                    } else {
                        grouped_target_cap_pools[cap_pool_i]
                            .insert(target.node_id, vec![target_id]);
                    }
                }

                // Only collect buddy groups belonging to the current pool
                for group in buddy_groups.iter().filter(|g| g.pool_id == pool.pool_id) {
                    buddy_group_vec.push(group.entity_id);
                    buddy_group_cap_pools[usize::from(group.cap_pool)].push(group.entity_id);
                }

                Ok(msg::types::StoragePool {
                    id: pool.pool_id,
                    alias: pool.alias.into_bytes(),
                    targets: target_map.keys().cloned().collect(),
                    buddy_groups: buddy_group_vec,
                    target_cap_pools: TargetCapacityPools {
                        pools: target_cap_pools.into(),
                        grouped_target_pools: grouped_target_cap_pools.into(),
                        target_map,
                    },
                    buddy_cap_pools: BuddyGroupCapacityPools {
                        pools: buddy_group_cap_pools.into(),
                    },
                })
            })
            .try_collect::<Vec<msg::types::StoragePool>>() as Result<_>
    }
    .await
    {
        Ok(pools) => pools,
        Err(err) => {
            log_error_chain!(err, "Getting storage pools failed");
            vec![]
        }
    };

    msg::GetStoragePoolsResp { pools }
}
