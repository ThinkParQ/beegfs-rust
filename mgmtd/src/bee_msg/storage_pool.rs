use super::*;
use crate::cap_pool::{CapPoolCalculator, CapacityInfo};
use shared::bee_msg::storage_pool::*;
use shared::types::{BuddyGroupId, NodeId, PoolId, TargetId};

struct TargetOrBuddyGroup {
    id: u16,
    node_id: Option<NodeId>,
    pool_id: PoolId,
    free_space: u64,
    free_inodes: u64,
}

impl CapacityInfo for &TargetOrBuddyGroup {
    fn free_space(&self) -> u64 {
        self.free_space
    }

    fn free_inodes(&self) -> u64 {
        self.free_inodes
    }
}

impl HandleWithResponse for GetStoragePools {
    type Response = GetStoragePoolsResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Result<Self::Response> {
        let (pools, targets, buddy_groups) = ctx
            .db
            .op(move |tx| {
                let pools: Vec<(PoolId, String)> = tx.query_map_collect(
                    sql!(
                        "SELECT pool_id, alias FROM storage_pools
                        INNER JOIN entities ON uid = pool_uid"
                    ),
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;

                let targets: Vec<TargetOrBuddyGroup> = tx.query_map_collect(
                    sql!(
                        "SELECT target_id, node_id, pool_id, free_space, free_inodes
                        FROM storage_targets
                        WHERE node_id IS NOT NULL
                            AND free_space IS NOT NULL AND free_inodes IS NOT NULL"
                    ),
                    [],
                    |row| {
                        Ok(TargetOrBuddyGroup {
                            id: row.get(0)?,
                            node_id: Some(row.get(1)?),
                            pool_id: row.get(2)?,
                            free_space: row.get(3)?,
                            free_inodes: row.get(4)?,
                        })
                    },
                )?;

                let buddy_groups: Vec<TargetOrBuddyGroup> = tx.query_map_collect(
                    sql!(
                        "SELECT group_id, g.pool_id,
                            MIN(p_t.free_space, s_t.free_space),
                            MIN(p_t.free_inodes, s_t.free_inodes)
                        FROM storage_buddy_groups AS g
                        INNER JOIN targets AS p_t ON p_t.target_id = g.p_target_id
                            AND p_t.node_type = g.node_type
                        INNER JOIN targets AS s_t ON s_t.target_id = g.s_target_id
                            AND s_t.node_type = g.node_type
                        WHERE p_t.free_space IS NOT NULL
                            AND s_t.free_space IS NOT NULL
                            AND p_t.free_inodes IS NOT NULL
                            AND s_t.free_inodes IS NOT NULL"
                    ),
                    [],
                    |row| {
                        Ok(TargetOrBuddyGroup {
                            id: row.get(0)?,
                            node_id: None,
                            pool_id: row.get(1)?,
                            free_space: row.get(2)?,
                            free_inodes: row.get(3)?,
                        })
                    },
                )?;

                Ok((pools, targets, buddy_groups))
            })
            .await?;

        // Build the data structures GetStoragePool wants, per pool
        let pools = pools
            .into_iter()
            .map(|pool| {
                // IDs belonging to the three cap pools
                let mut target_cap_pools = [vec![], vec![], vec![]];
                let mut buddy_group_cap_pools = [vec![], vec![], vec![]];

                // NodeID -> Vec<TargetID> map for each cap pool
                let mut grouped_target_cap_pools = [
                    HashMap::<NodeId, Vec<TargetId>>::new(),
                    HashMap::new(),
                    HashMap::new(),
                ];

                // Target / buddy group info without cap pools
                let mut target_map: HashMap<TargetId, NodeId> = HashMap::new();
                let mut buddy_group_vec: Vec<BuddyGroupId> = vec![];

                let f_targets = targets.iter().filter(|t| t.pool_id == pool.0);
                let f_buddy_groups = buddy_groups.iter().filter(|t| t.pool_id == pool.0);

                let cp_targets_calc = CapPoolCalculator::new(
                    ctx.info.user_config.cap_pool_storage_limits.clone(),
                    ctx.info
                        .user_config
                        .cap_pool_dynamic_storage_limits
                        .as_ref(),
                    f_targets.clone(),
                )?;

                let cp_buddy_groups_calc = CapPoolCalculator::new(
                    ctx.info.user_config.cap_pool_storage_limits.clone(),
                    ctx.info
                        .user_config
                        .cap_pool_dynamic_storage_limits
                        .as_ref(),
                    f_buddy_groups.clone(),
                )?;

                // Only collect targets belonging to the current pool
                for target in f_targets {
                    let cp = cp_targets_calc
                        .cap_pool(target.free_space, target.free_inodes)
                        .bee_msg_vec_index();

                    let target_id: TargetId = target.id;
                    let node_id = target.node_id.expect("targets have a node id");

                    target_map.insert(target_id, node_id);
                    target_cap_pools[cp].push(target.id);

                    if let Some(node_group) = grouped_target_cap_pools[cp].get_mut(&node_id) {
                        node_group.push(target_id);
                    } else {
                        grouped_target_cap_pools[cp].insert(node_id, vec![target_id]);
                    }
                }

                // Only collect buddy groups belonging to the current pool
                for group in f_buddy_groups {
                    buddy_group_vec.push(group.id);

                    let cp = cp_buddy_groups_calc
                        .cap_pool(group.free_space, group.free_inodes)
                        .bee_msg_vec_index();
                    buddy_group_cap_pools[cp].push(group.id);
                }

                Ok(StoragePool {
                    id: pool.0,
                    alias: pool.1.into_bytes(),
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
            .collect::<Result<Vec<StoragePool>>>()?;

        Ok(GetStoragePoolsResp { pools })
    }
}
