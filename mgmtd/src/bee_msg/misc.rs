use super::*;
use crate::cap_pool::{CapPoolCalculator, CapacityInfo};
use rusqlite::Transaction;
use shared::bee_msg::misc::*;
use shared::types::PoolId;
use sqlite::TransactionExt;
use sqlite_check::sql;

impl Handler for Ack {
    type Response = ();

    async fn handle(self, _ctx: &Context, req: &mut impl Request) -> Self::Response {
        log::debug!("Ignoring Ack from {:?}: Id: {:?}", req.addr(), self.ack_id);
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
        // This message is superfluous and therefore ignored. It is meant to tell the
        // mgmtd to trigger a capacity pool pull immediately after a node starts.
        // meta and storage send a SetTargetInfo before this msg though,
        // so we handle triggering pulls there.

        Ack {
            ack_id: self.ack_id,
        }
    }
}

struct TargetOrBuddyGroup {
    id: u16,
    pool_id: Option<PoolId>,
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

fn load_targets_by_type(
    tx: &Transaction,
    node_type: NodeTypeServer,
) -> Result<Vec<TargetOrBuddyGroup>> {
    let targets = tx.query_map_collect(
        sql!(
            "SELECT target_id, pool_id, free_space, free_inodes
            FROM all_targets_v
            WHERE node_type = ?1 AND free_space IS NOT NULL AND free_inodes IS NOT NULL"
        ),
        [node_type.sql_str()],
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

fn load_buddy_groups_by_type(
    tx: &Transaction,
    node_type: NodeTypeServer,
) -> Result<Vec<TargetOrBuddyGroup>> {
    let groups = tx.query_map_collect(
        sql!(
            "SELECT group_id, pool_id,
                MIN(p_t.free_space, s_t.free_space),
                MIN(p_t.free_inodes, s_t.free_inodes)
            FROM all_buddy_groups_v AS g
            INNER JOIN targets AS p_t ON p_t.target_uid = p_target_uid
            INNER JOIN targets AS s_t ON s_t.target_uid = s_target_uid
            WHERE g.node_type = ?1
                AND p_t.free_space IS NOT NULL AND s_t.free_space IS NOT NULL
                AND p_t.free_inodes IS NOT NULL AND s_t.free_inodes IS NOT NULL"
        ),
        [node_type.sql_str()],
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

impl Handler for GetNodeCapacityPools {
    type Response = GetNodeCapacityPoolsResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        let res = async move {
            // We return raw u16 here as ID because BeeGFS expects a u16 that can be
            // either a NodeNUmID, TargetNumID or BuddyGroupID

            let result: HashMap<PoolId, Vec<Vec<u16>>> = match self.query_type {
                CapacityPoolQueryType::Meta => {
                    let targets = ctx
                        .db
                        .op(|tx| load_targets_by_type(tx, NodeTypeServer::Meta))
                        .await?;

                    let cp_calc = CapPoolCalculator::new(
                        ctx.info.user_config.cap_pool_meta_limits.clone(),
                        ctx.info.user_config.cap_pool_dynamic_meta_limits.as_ref(),
                        &targets,
                    )?;

                    let mut res = vec![Vec::<u16>::new(), vec![], vec![]];
                    for t in targets {
                        let cp = cp_calc
                            .cap_pool(t.free_space, t.free_inodes)
                            .bee_msg_vec_index();
                        res[cp].push(t.id);
                    }

                    [(0, res)].into()
                }

                CapacityPoolQueryType::Storage => {
                    let (targets, pools) = ctx
                        .db
                        .op(|tx| {
                            let targets = load_targets_by_type(tx, NodeTypeServer::Storage)?;

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
                                .cap_pool(t.free_space, t.free_inodes)
                                .bee_msg_vec_index();
                            res.get_mut(&sp).unwrap()[cp].push(t.id);
                        }
                    }

                    res
                }

                CapacityPoolQueryType::MetaMirrored => {
                    let groups = ctx
                        .db
                        .op(|tx| load_buddy_groups_by_type(tx, NodeTypeServer::Meta))
                        .await?;

                    let cp_calc = CapPoolCalculator::new(
                        ctx.info.user_config.cap_pool_meta_limits.clone(),
                        ctx.info.user_config.cap_pool_dynamic_meta_limits.as_ref(),
                        &groups,
                    )?;

                    let mut res = vec![Vec::<u16>::new(), vec![], vec![]];

                    for e in groups {
                        let cp = cp_calc
                            .cap_pool(e.free_space, e.free_inodes)
                            .bee_msg_vec_index();
                        res[cp].push(e.id);
                    }

                    [(0, res)].into()
                }

                CapacityPoolQueryType::StorageMirrored => {
                    let (groups, pools) = ctx
                        .db
                        .op(|tx| {
                            let groups = load_buddy_groups_by_type(tx, NodeTypeServer::Storage)?;

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
                                .cap_pool(t.free_space, t.free_inodes)
                                .bee_msg_vec_index();
                            cap_pools.get_mut(&sp).unwrap()[cp].push(t.id);
                        }
                    }

                    cap_pools
                }
            };

            Ok(result) as Result<_>
        }
        .await;

        let pools = match res {
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
