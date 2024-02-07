use super::*;
use shared::bee_msg::misc::*;
use shared::types::StoragePoolID;

impl Handler for Ack {
    type Response = ();

    async fn handle(self, _ctx: &Context, req: &mut impl Request) -> Self::Response {
        log::debug!("Ignoring Ack from {:?}: ID: {:?}", req.addr(), self.ack_id);
        todo!()
    }
}

impl Handler for AuthenticateChannel {
    type Response = ();

    async fn handle(self, ctx: &Context, req: &mut impl Request) -> Self::Response {
        if let Some(ref secret) = ctx.info.auth_secret {
            if secret == &self.auth_secret {
                req.authenticate_connection();
            } else {
                log::error!(
                    "Peer {:?} tried to authenticate stream with wrong secret",
                    req.addr()
                );
            }
        } else {
            log::debug!(
                "Peer {:?} tried to authenticate stream, but authentication is not required",
                req.addr()
            );
        }
    }
}

impl Handler for PeerInfo {
    type Response = ();

    async fn handle(self, _ctx: &Context, _req: &mut impl Request) -> Self::Response {
        // This is supposed to give some information about a connection, but it looks
        // like this isnt used at all
    }
}

impl Handler for SetChannelDirect {
    type Response = ();

    async fn handle(self, _ctx: &Context, _req: &mut impl Request) -> Self::Response {
        // do nothing
    }
}

impl Handler for RefreshCapacityPools {
    type Response = Ack;

    async fn handle(self, _ctx: &Context, _req: &mut impl Request) -> Self::Response {
        // This message is superfluos and therefore ignored. It is meant to tell the
        // mgmtd to trigger a capacity pool pull immediately after a node starts.
        // meta and storage send a SetTargetInfo before this msg though,
        // so we handle triggering pulls there.

        Ack {
            ack_id: self.ack_id,
        }
    }
}

impl Handler for GetNodeCapacityPools {
    type Response = GetNodeCapacityPoolsResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        let pools = match async move {
            // We return raw u16 here as ID because BeeGFS expects a u16 that can be
            // either a NodeNUmID, TargetNumID or BuddyGroupID

            let config = &ctx.info.user_config;

            let result: HashMap<StoragePoolID, Vec<Vec<u16>>> = match self.query_type {
                CapacityPoolQueryType::Meta => {
                    let res = ctx
                        .db
                        .op(|tx| {
                            db::cap_pool::for_meta_targets(
                                tx,
                                &config.cap_pool_meta_limits,
                                config.cap_pool_dynamic_meta_limits.as_ref(),
                            )
                        })
                        .await?;

                    let mut target_cap_pools = vec![Vec::<u16>::new(), vec![], vec![]];

                    for t in res {
                        target_cap_pools[usize::from(CapacityPool::from(t.cap_pool))]
                            .push(t.entity_id);
                    }

                    [(0, target_cap_pools)].into()
                }
                CapacityPoolQueryType::Storage => {
                    let res = ctx
                        .db
                        .op(|tx| {
                            db::cap_pool::for_storage_targets(
                                tx,
                                &config.cap_pool_storage_limits,
                                config.cap_pool_dynamic_storage_limits.as_ref(),
                            )
                        })
                        .await?;

                    let mut group_cap_pools: HashMap<StoragePoolID, Vec<Vec<u16>>> = HashMap::new();
                    for t in res {
                        if let Some(pool_groups) = group_cap_pools.get_mut(&t.pool_id) {
                            pool_groups[usize::from(CapacityPool::from(t.cap_pool))]
                                .push(t.entity_id);
                        } else {
                            let mut pool_groups = [vec![], vec![], vec![]];
                            pool_groups[usize::from(CapacityPool::from(t.cap_pool))]
                                .push(t.entity_id);
                            group_cap_pools.insert(t.pool_id, pool_groups.into());
                        }
                    }

                    group_cap_pools
                }

                CapacityPoolQueryType::MetaMirrored => {
                    let res = ctx
                        .db
                        .op(|tx| {
                            db::cap_pool::for_meta_buddy_groups(
                                tx,
                                &config.cap_pool_meta_limits,
                                config.cap_pool_dynamic_meta_limits.as_ref(),
                            )
                        })
                        .await?;

                    let mut group_cap_pools = vec![Vec::<u16>::new(), vec![], vec![]];
                    for g in res {
                        group_cap_pools[usize::from(CapacityPool::from(g.cap_pool))]
                            .push(g.entity_id);
                    }

                    [(0, group_cap_pools)].into()
                }

                CapacityPoolQueryType::StorageMirrored => {
                    let res = ctx
                        .db
                        .op(|tx| {
                            db::cap_pool::for_storage_buddy_groups(
                                tx,
                                &config.cap_pool_storage_limits,
                                config.cap_pool_dynamic_storage_limits.as_ref(),
                            )
                        })
                        .await?;

                    let mut group_cap_pools: HashMap<StoragePoolID, Vec<Vec<u16>>> = HashMap::new();
                    for g in res {
                        if let Some(pool_groups) = group_cap_pools.get_mut(&g.pool_id) {
                            pool_groups[usize::from(CapacityPool::from(g.cap_pool))]
                                .push(g.entity_id);
                        } else {
                            let mut pool_groups = [vec![], vec![], vec![]];
                            pool_groups[usize::from(CapacityPool::from(g.cap_pool))]
                                .push(g.entity_id);
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
                log_error_chain!(
                    err,
                    "Getting node capacity pools with query type {:?} failed",
                    self.query_type
                );

                HashMap::new()
            }
        };

        GetNodeCapacityPoolsResp { pools }
    }
}
