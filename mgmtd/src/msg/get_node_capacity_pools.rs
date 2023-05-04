use super::*;
use crate::logic;
use shared::msg::types::CapacityPoolQueryType;

pub(super) async fn handle(
    msg: msg::GetNodeCapacityPools,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    let pools = match async move {
        // We return a raw u16 here as ID because BeeGFS expects a u16 that can be
        // either a NodeNUmID, TargetNumID or BuddyGroupID here in the new code.

        let result: HashMap<StoragePoolID, Vec<Vec<u16>>> = match msg.query_type {
            CapacityPoolQueryType::Meta => {
                let targets = hnd
                    .execute_db(move |tx| db::targets::with_type(tx, NodeTypeServer::Meta))
                    .await?;

                let mut target_cap_pools = vec![Vec::<u16>::new(), vec![], vec![]];
                let cap_pool_cfg = hnd.get_config::<config::CapPoolMetaLimits>();

                for t in &targets {
                    let cap_pool =
                        logic::calc_cap_pool(&cap_pool_cfg, t.free_space, t.free_inodes) as usize;

                    target_cap_pools[cap_pool].push(t.target_id.into());
                }

                [(StoragePoolID::ZERO, target_cap_pools)].into()
            }
            CapacityPoolQueryType::Storage => {
                let targets = hnd
                    .execute_db(move |tx| db::targets::with_type(tx, NodeTypeServer::Storage))
                    .await?;

                let mut group_cap_pools: HashMap<StoragePoolID, Vec<Vec<u16>>> = HashMap::new();
                for t in &targets {
                    let cap_pool = logic::calc_cap_pool(
                        &hnd.get_config::<config::CapPoolStorageLimits>(),
                        t.free_space,
                        t.free_inodes,
                    ) as usize;

                    if let Some(pool_groups) = group_cap_pools.get_mut(
                        &t.pool_id
                            .ok_or_else(|| anyhow!("Missing pool_id on storage target"))?,
                    ) {
                        pool_groups[cap_pool].push(t.target_id.into());
                    } else {
                        let mut pool_groups = [vec![], vec![], vec![]];
                        pool_groups[cap_pool].push(t.target_id.into());
                        group_cap_pools.insert(
                            t.pool_id
                                .ok_or_else(|| anyhow!("Missing pool_id on storage target"))?,
                            pool_groups.into(),
                        );
                    }
                }

                group_cap_pools
            }

            CapacityPoolQueryType::MetaMirrored => {
                let hnd2 = hnd.clone();
                let groups = hnd
                    .execute_db(move |tx| db::buddy_groups::with_type(tx, NodeTypeServer::Meta))
                    .await?;

                let mut group_cap_pools = vec![Vec::<u16>::new(), vec![], vec![]];
                for g in groups.into_iter() {
                    let cap_pool_1 = logic::calc_cap_pool(
                        &hnd2.get_config::<config::CapPoolMetaLimits>(),
                        g.primary_free_space,
                        g.primary_free_inodes,
                    );

                    let cap_pool_2 = logic::calc_cap_pool(
                        &hnd2.get_config::<config::CapPoolMetaLimits>(),
                        g.secondary_free_space,
                        g.secondary_free_inodes,
                    );

                    let lowest_cap_pool = CapacityPool::lowest(cap_pool_1, cap_pool_2);

                    group_cap_pools[lowest_cap_pool as usize].push(g.id.into());
                }

                [(StoragePoolID::ZERO, group_cap_pools)].into()
            }

            CapacityPoolQueryType::StorageMirrored => {
                let hnd2 = hnd.clone();
                let groups = hnd
                    .execute_db(move |tx| db::buddy_groups::with_type(tx, NodeTypeServer::Storage))
                    .await?;

                let mut group_cap_pools: HashMap<StoragePoolID, Vec<Vec<u16>>> = HashMap::new();
                for g in groups.into_iter() {
                    let cap_pool_1 = logic::calc_cap_pool(
                        &hnd2.get_config::<config::CapPoolStorageLimits>(),
                        g.primary_free_space,
                        g.primary_free_inodes,
                    );

                    let cap_pool_2 = logic::calc_cap_pool(
                        &hnd2.get_config::<config::CapPoolStorageLimits>(),
                        g.secondary_free_space,
                        g.secondary_free_inodes,
                    );

                    let lowest_cap_pool = CapacityPool::lowest(cap_pool_1, cap_pool_2);

                    let pool_id = g.pool_id.try_into()?;

                    if let Some(pool_groups) = group_cap_pools.get_mut(&pool_id) {
                        pool_groups[lowest_cap_pool as usize].push(g.id.into());
                    } else {
                        let mut pool_groups = [vec![], vec![], vec![]];
                        pool_groups[lowest_cap_pool as usize].push(u16::from(g.id));
                        group_cap_pools.insert(pool_id, pool_groups.into());
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
