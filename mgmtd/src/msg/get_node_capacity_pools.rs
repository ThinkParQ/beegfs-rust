use super::*;
use shared::msg::types::CapacityPoolQueryType;

pub(super) async fn handle(
    msg: msg::GetNodeCapacityPools,
    ctx: &impl AppContext,
    _req: &impl Request,
) -> msg::GetNodeCapacityPoolsResp {
    let pools = match async move {
        // We return raw u16 here as ID because BeeGFS expects a u16 that can be
        // either a NodeNUmID, TargetNumID or BuddyGroupID

        let result: HashMap<StoragePoolID, Vec<Vec<u16>>> = match msg.query_type {
            CapacityPoolQueryType::Meta => {
                let res = ctx.db_op(db::cap_pool::for_meta_targets).await?;

                let mut target_cap_pools = vec![Vec::<u16>::new(), vec![], vec![]];

                for t in res {
                    target_cap_pools[usize::from(t.cap_pool)].push(t.entity_id);
                }

                [(StoragePoolID::ZERO, target_cap_pools)].into()
            }
            CapacityPoolQueryType::Storage => {
                let res = ctx.db_op(db::cap_pool::for_storage_targets).await?;

                let mut group_cap_pools: HashMap<StoragePoolID, Vec<Vec<u16>>> = HashMap::new();
                for t in res {
                    if let Some(pool_groups) = group_cap_pools.get_mut(&t.pool_id) {
                        pool_groups[usize::from(t.cap_pool)].push(t.entity_id);
                    } else {
                        let mut pool_groups = [vec![], vec![], vec![]];
                        pool_groups[usize::from(t.cap_pool)].push(t.entity_id);
                        group_cap_pools.insert(t.pool_id, pool_groups.into());
                    }
                }

                group_cap_pools
            }

            CapacityPoolQueryType::MetaMirrored => {
                let res = ctx.db_op(db::cap_pool::for_meta_buddy_groups).await?;

                let mut group_cap_pools = vec![Vec::<u16>::new(), vec![], vec![]];
                for g in res {
                    group_cap_pools[usize::from(g.cap_pool)].push(g.entity_id);
                }

                [(StoragePoolID::ZERO, group_cap_pools)].into()
            }

            CapacityPoolQueryType::StorageMirrored => {
                let res = ctx.db_op(db::cap_pool::for_storage_buddy_groups).await?;

                let mut group_cap_pools: HashMap<StoragePoolID, Vec<Vec<u16>>> = HashMap::new();
                for g in res {
                    if let Some(pool_groups) = group_cap_pools.get_mut(&g.pool_id) {
                        pool_groups[usize::from(g.cap_pool)].push(g.entity_id);
                    } else {
                        let mut pool_groups = [vec![], vec![], vec![]];
                        pool_groups[usize::from(g.cap_pool)].push(g.entity_id);
                        group_cap_pools.insert(g.pool_id, pool_groups.into());
                    }
                }

                group_cap_pools
            }
        };

        Ok(result) as DbResult<_>
    }
    .await
    {
        Ok(pools) => pools,
        Err(err) => {
            log_error_chain!(
                err,
                "Getting node capacity pools with query type {:?} failed",
                msg.query_type
            );

            HashMap::new()
        }
    };

    msg::GetNodeCapacityPoolsResp { pools }
}
