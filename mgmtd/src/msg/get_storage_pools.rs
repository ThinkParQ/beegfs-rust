use super::*;
use shared::msg::types::{BuddyGroupCapacityPools, TargetCapacityPools};

pub(super) async fn handle(
    _msg: msg::GetStoragePools,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    let pools = match async move {
        let (targets, pools, buddy_groups) = hnd
            .execute_db(move |tx| {
                let targets = db::targets::with_type(tx, NodeTypeServer::Storage)?;
                let pools = db::storage_pools::all(tx)?;
                let buddy_groups = db::buddy_groups::with_type(tx, NodeTypeServer::Storage)?;

                Ok((targets, pools, buddy_groups))
            })
            .await?;

        // Build all the information BeeGFS wants per pool
        // This is a hot mess, contains redundant information and needs to be cleaned up
        // - here and in the C++ code
        pools
            .into_iter()
            .map(|p| {
                let mut target_cap_pools = [vec![], vec![], vec![]];
                let mut buddy_group_cap_pools = [vec![], vec![], vec![]];

                let mut grouped_target_cap_pools = [
                    HashMap::<NodeID, Vec<TargetID>>::new(),
                    HashMap::new(),
                    HashMap::new(),
                ];

                let mut target_map = HashMap::<TargetID, NodeID>::new();

                // Go through all targets
                for t in &targets {
                    // Only process these belonging to the current pool
                    if StoragePoolID::try_from(t.pool_id)? != p.pool_id {
                        continue;
                    }

                    let cap_pool = logic::calc_cap_pool(
                        &hnd.get_config::<config::CapPoolStorageLimits>(),
                        t.free_space,
                        t.free_inodes,
                    ) as usize;

                    // This just builds a map with all targets to their node
                    target_map.insert(t.target_id, t.node_id);

                    // This builds an array with the three cap pools, each containing a map
                    // mapping the node the target is on to a vec of all the targets on that node
                    if let Some(node_group) = grouped_target_cap_pools[cap_pool].get_mut(&t.node_id)
                    {
                        node_group.push(t.target_id);
                    } else {
                        grouped_target_cap_pools[cap_pool].insert(t.node_id, vec![t.target_id]);
                    }

                    // This just builds a vec of targets belonging to each cap pool
                    target_cap_pools[cap_pool].push(t.target_id);

                    // If this target belongs to a buddy group and is the primary
                    // we add the corresponding group id to the group cap pools
                    // TODO use both primary and secondary target to determine the pool (use worst)
                    if let Some(buddy_group) = buddy_groups
                        .iter()
                        .find(|e| t.target_id == e.primary_target_id)
                    {
                        if StoragePoolID::try_from(buddy_group.pool_id)? == p.pool_id {
                            buddy_group_cap_pools[cap_pool].push(buddy_group.id);
                        }
                    }
                }

                let mut assigned_buddy_groups = vec![];

                for g in &buddy_groups {
                    if StoragePoolID::try_from(g.pool_id)? != p.pool_id {
                        continue;
                    }

                    // Add buddy group that is assigned to current pool to vec
                    assigned_buddy_groups.push(g.id);
                }

                Ok(msg::types::StoragePool {
                    id: p.pool_id,
                    alias: p.alias,
                    targets: target_map.keys().cloned().collect(),
                    buddy_groups: assigned_buddy_groups,
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
            log::error!("Getting storage pools failed:\n{err:?}");
            vec![]
        }
    };

    chn.respond(&msg::GetStoragePoolsResp { pools }).await
}
