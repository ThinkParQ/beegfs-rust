use super::*;
use crate::cap_pool::{CapPoolCalculator, CapacityInfo};
use rusqlite::Transaction;
use shared::bee_msg::misc::*;

#[derive(Debug)]
struct TargetOrBuddyGroup {
    id: u16,
    pool_id: Option<PoolId>,
    free_space: Option<u64>,
    free_inodes: Option<u64>,
}

impl CapacityInfo for &TargetOrBuddyGroup {
    fn free_space(&self) -> u64 {
        self.free_space.unwrap_or_default()
    }

    fn free_inodes(&self) -> u64 {
        self.free_inodes.unwrap_or_default()
    }
}

fn load_targets_info_by_type(
    tx: &Transaction,
    node_type: NodeTypeServer,
) -> Result<Vec<TargetOrBuddyGroup>> {
    let targets = tx.query_map_collect(
        sql!(
            "SELECT target_id, pool_id, free_space, free_inodes
            FROM targets
            WHERE node_type = ?1"
        ),
        [node_type.sql_variant()],
        |row| {
            Ok(TargetOrBuddyGroup {
                id: row.get(0)?,
                pool_id: row.get(1)?,
                free_space: row.get(2)?,
                free_inodes: row.get(3)?,
            })
        },
    )?;

    Ok(targets)
}

fn load_buddy_groups_info_by_type(
    tx: &Transaction,
    node_type: NodeTypeServer,
) -> Result<Vec<TargetOrBuddyGroup>> {
    let groups = tx.query_map_collect(
        sql!(
            "SELECT g.group_id, g.pool_id,
                MIN(p_t.free_space, s_t.free_space),
                MIN(p_t.free_inodes, s_t.free_inodes)
            FROM buddy_groups_ext AS g
            INNER JOIN targets AS p_t ON p_t.target_uid = g.p_target_uid AND p_t.node_type = g.node_type
            INNER JOIN targets AS s_t ON s_t.target_uid = g.s_target_uid AND s_t.node_type = g.node_type
            WHERE g.node_type = ?1"
        ),
        [node_type.sql_variant()],
        |row| {
            Ok(TargetOrBuddyGroup {
                id: row.get(0)?,
                pool_id: row.get(1)?,
                free_space: row.get(2)?,
                free_inodes: row.get(3)?,
            })
        },
    )?;

    Ok(groups)
}

impl HandleWithResponse for GetNodeCapacityPools {
    type Response = GetNodeCapacityPoolsResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Result<Self::Response> {
        // We return raw u16 here as ID because BeeGFS expects a u16 that can be
        // either a NodeNUmID, TargetNumID or BuddyGroupID

        let pools: HashMap<PoolId, Vec<Vec<u16>>> = match self.query_type {
            CapacityPoolQueryType::Meta => {
                let targets = ctx
                    .db
                    .read_tx(|tx| load_targets_info_by_type(tx, NodeTypeServer::Meta))
                    .await?;

                let cp_calc = CapPoolCalculator::new(
                    ctx.info.user_config.cap_pool_meta_limits.clone(),
                    ctx.info.user_config.cap_pool_dynamic_meta_limits.as_ref(),
                    &targets,
                )?;

                let mut res = vec![Vec::<u16>::new(), vec![], vec![]];
                for t in &targets {
                    let cp = cp_calc
                        .cap_pool(t.free_space(), t.free_inodes())
                        .bee_msg_vec_index();
                    res[cp].push(t.id);
                }

                [(0, res)].into()
            }

            CapacityPoolQueryType::Storage => {
                let (targets, pools) = ctx
                    .db
                    .read_tx(|tx| {
                        let targets = load_targets_info_by_type(tx, NodeTypeServer::Storage)?;

                        let pools: Vec<PoolId> = tx.query_map_collect(
                            sql!("SELECT pool_id FROM storage_pools"),
                            [],
                            |row| row.get(0),
                        )?;

                        Ok((targets, pools))
                    })
                    .await?;

                let mut res: HashMap<PoolId, Vec<Vec<u16>>> = HashMap::new();
                for sp in pools {
                    let f_targets = targets.iter().filter(|e| e.pool_id == Some(sp));

                    let cp_calc = CapPoolCalculator::new(
                        ctx.info.user_config.cap_pool_storage_limits.clone(),
                        ctx.info
                            .user_config
                            .cap_pool_dynamic_storage_limits
                            .as_ref(),
                        f_targets.clone(),
                    )?;

                    res.insert(sp, vec![Vec::<u16>::new(), vec![], vec![]]);
                    for t in f_targets {
                        let cp = cp_calc
                            .cap_pool(t.free_space(), t.free_inodes())
                            .bee_msg_vec_index();
                        res.get_mut(&sp).unwrap()[cp].push(t.id);
                    }
                }

                res
            }

            CapacityPoolQueryType::MetaMirrored => {
                let groups = ctx
                    .db
                    .read_tx(|tx| load_buddy_groups_info_by_type(tx, NodeTypeServer::Meta))
                    .await?;

                let cp_calc = CapPoolCalculator::new(
                    ctx.info.user_config.cap_pool_meta_limits.clone(),
                    ctx.info.user_config.cap_pool_dynamic_meta_limits.as_ref(),
                    &groups,
                )?;

                let mut res = vec![Vec::<u16>::new(), vec![], vec![]];

                for e in &groups {
                    let cp = cp_calc
                        .cap_pool(e.free_space(), e.free_inodes())
                        .bee_msg_vec_index();
                    res[cp].push(e.id);
                }

                [(0, res)].into()
            }

            CapacityPoolQueryType::StorageMirrored => {
                let (groups, pools) = ctx
                    .db
                    .read_tx(|tx| {
                        let groups = load_buddy_groups_info_by_type(tx, NodeTypeServer::Storage)?;

                        let pools: Vec<PoolId> = tx.query_map_collect(
                            sql!("SELECT pool_id FROM storage_pools"),
                            [],
                            |row| row.get(0),
                        )?;

                        Ok((groups, pools))
                    })
                    .await?;

                let mut cap_pools: HashMap<PoolId, Vec<Vec<u16>>> = HashMap::new();
                for sp in pools {
                    let f_groups = groups.iter().filter(|e| e.pool_id == Some(sp));

                    let cp_calc = CapPoolCalculator::new(
                        ctx.info.user_config.cap_pool_storage_limits.clone(),
                        ctx.info
                            .user_config
                            .cap_pool_dynamic_storage_limits
                            .as_ref(),
                        f_groups.clone(),
                    )?;

                    cap_pools.insert(sp, vec![Vec::<u16>::new(), vec![], vec![]]);
                    for t in f_groups {
                        let cp = cp_calc
                            .cap_pool(t.free_space(), t.free_inodes())
                            .bee_msg_vec_index();
                        cap_pools.get_mut(&sp).unwrap()[cp].push(t.id);
                    }
                }

                cap_pools
            }
        };

        Ok(GetNodeCapacityPoolsResp { pools })
    }
}
