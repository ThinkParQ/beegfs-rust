use super::*;
use shared::msg::types::CapacityPoolQueryType;

pub(super) async fn handle(
    msg: msg::GetNodeCapacityPools,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    let pools = match async move {
        // We return raw u16 here as ID because BeeGFS expects a u16 that can be
        // either a NodeNUmID, TargetNumID or BuddyGroupID

        let result: HashMap<StoragePoolID, Vec<Vec<u16>>> = match msg.query_type {
            CapacityPoolQueryType::Meta => {
                let cap_pool_cfg = hnd.get_config::<config::CapPoolMetaLimits>();
                let cap_pool_dynamic_cfg = hnd.get_config::<config::CapPoolDynamicMetaLimits>();

                let res = hnd
                    .execute_db(move |tx| {
                        db::cap_pools::for_meta_targets(tx, cap_pool_cfg, cap_pool_dynamic_cfg)
                    })
                    .await?;

                let mut target_cap_pools = vec![Vec::<u16>::new(), vec![], vec![]];

                for t in res {
                    target_cap_pools[usize::from(t.cap_pool)].push(t.entity_id);
                }

                [(StoragePoolID::ZERO, target_cap_pools)].into()
            }
            CapacityPoolQueryType::Storage => {
                let cap_pool_cfg = hnd.get_config::<config::CapPoolStorageLimits>();
                let cap_pool_dynamic_cfg = hnd.get_config::<config::CapPoolDynamicStorageLimits>();

                let res = hnd
                    .execute_db(move |tx| {
                        db::cap_pools::for_storage_targets(tx, cap_pool_cfg, cap_pool_dynamic_cfg)
                    })
                    .await?;

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
                let cap_pool_cfg = hnd.get_config::<config::CapPoolMetaLimits>();
                let cap_pool_dynamic_cfg = hnd.get_config::<config::CapPoolDynamicMetaLimits>();

                let res = hnd
                    .execute_db(move |tx| {
                        db::cap_pools::for_meta_buddy_groups(tx, cap_pool_cfg, cap_pool_dynamic_cfg)
                    })
                    .await?;

                let mut group_cap_pools = vec![Vec::<u16>::new(), vec![], vec![]];
                for g in res {
                    group_cap_pools[usize::from(g.cap_pool)].push(g.entity_id);
                }

                [(StoragePoolID::ZERO, group_cap_pools)].into()
            }

            CapacityPoolQueryType::StorageMirrored => {
                let cap_pool_cfg = hnd.get_config::<config::CapPoolStorageLimits>();
                let cap_pool_dynamic_cfg = hnd.get_config::<config::CapPoolDynamicStorageLimits>();

                let res = hnd
                    .execute_db(move |tx| {
                        db::cap_pools::for_storage_buddy_groups(
                            tx,
                            cap_pool_cfg,
                            cap_pool_dynamic_cfg,
                        )
                    })
                    .await?;

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

        Ok(result) as Result<_>
    }
    .await
    {
        Ok(pools) => pools,
        Err(err) => {
            log::error!(
                "Getting node capacity pools with query type {:?} failed:\n{:?}",
                msg.query_type,
                err
            );
            HashMap::new()
        }
    };

    chn.respond(&msg::GetNodeCapacityPoolsResp { pools }).await
}
