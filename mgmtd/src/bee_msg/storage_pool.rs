use super::*;
use crate::cap_pool::{CapPoolCalculator, CapacityInfo};
use shared::bee_msg::storage_pool::*;
use shared::types::{BuddyGroupID, NodeID, StoragePoolID, TargetID, DEFAULT_STORAGE_POOL};

impl Handler for AddStoragePool {
    type Response = AddStoragePoolResp;

    async fn handle(self, ctx: &Context, __req: &mut impl Request) -> Self::Response {
        let res = ctx
            .db
            .op(move |tx| {
                let alias = &std::str::from_utf8(&self.alias)?;

                let (_, pool_id) = db::storage_pool::insert(tx, self.pool_id, alias)?;

                // Update storage pool assignments for the given targets
                db::target::update_storage_pools(tx, pool_id, &self.move_target_ids)?;
                db::buddy_group::update_storage_pools(tx, pool_id, &self.move_buddy_group_ids)?;

                Ok(pool_id)
            })
            .await;

        match res {
            Ok(actual_id) => {
                log::info!(
                    "Added new storage pool with ID {} (Requested: {})",
                    actual_id,
                    self.pool_id,
                );

                notify_nodes(
                    ctx,
                    &[NodeType::Meta, NodeType::Storage],
                    &RefreshStoragePools { ack_id: "".into() },
                )
                .await;

                AddStoragePoolResp {
                    result: OpsErr::SUCCESS,
                    pool_id: actual_id,
                }
            }
            Err(err) => {
                log_error_chain!(err, "Adding storage pool with ID {} failed", self.pool_id);

                AddStoragePoolResp {
                    result: match err.downcast_ref() {
                        Some(TypedError::ValueExists { .. }) => OpsErr::EXISTS,
                        _ => OpsErr::INTERNAL,
                    },
                    pool_id: 0,
                }
            }
        }
    }
}

struct TargetOrBuddyGroup {
    id: u16,
    node_id: Option<NodeID>,
    pool_id: StoragePoolID,
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

impl Handler for GetStoragePools {
    type Response = GetStoragePoolsResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        let res = async move {
            let (pools, targets, buddy_groups) = ctx
                .db
                .op(move |tx| {
                    let pools: Vec<(StoragePoolID, String)> = tx.query_map_collect(
                        sql!(
                            "SELECT p.pool_id, e.alias
                            FROM storage_pools AS p
                            INNER JOIN entities AS e ON e.uid = p.pool_uid"
                        ),
                        [],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )?;

                    let targets: Vec<TargetOrBuddyGroup> = tx.query_map_collect(
                        sql!(
                            "SELECT target_id, node_id, pool_id, free_space, free_inodes
                            FROM all_targets_v
                            WHERE node_type == 'storage'
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
                            "SELECT buddy_group_id, pool_id,
                                MIN(p_t.free_space, s_t.free_space),
                                MIN(p_t.free_inodes, s_t.free_inodes)
                            FROM all_buddy_groups_v AS g
                            INNER JOIN targets AS p_t ON p_t.target_uid = g.p_target_uid
                            INNER JOIN targets AS s_t ON s_t.target_uid = g.s_target_uid
                            WHERE g.node_type = 'storage'
                                AND p_t.free_space IS NOT NULL
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
                        let cap_pool_i = usize::from(
                            cp_targets_calc.cap_pool(target.free_space, target.free_inodes),
                        );

                        let target_id: TargetID = target.id;
                        let node_id = target.node_id.expect("targets have a node id");

                        target_map.insert(target_id, node_id);
                        target_cap_pools[cap_pool_i].push(target.id);

                        if let Some(node_group) =
                            grouped_target_cap_pools[cap_pool_i].get_mut(&node_id)
                        {
                            node_group.push(target_id);
                        } else {
                            grouped_target_cap_pools[cap_pool_i].insert(node_id, vec![target_id]);
                        }
                    }

                    // Only collect buddy groups belonging to the current pool
                    for group in f_buddy_groups {
                        buddy_group_vec.push(group.id);
                        buddy_group_cap_pools[usize::from(
                            cp_buddy_groups_calc.cap_pool(group.free_space, group.free_inodes),
                        )]
                        .push(group.id);
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
                .collect::<Result<Vec<StoragePool>>>() as Result<_>
        }
        .await;

        let pools = match res {
            Ok(pools) => pools,
            Err(err) => {
                log_error_chain!(err, "Getting storage pools failed");
                vec![]
            }
        };

        GetStoragePoolsResp { pools }
    }
}

impl Handler for ModifyStoragePool {
    type Response = ModifyStoragePoolResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        match async {
            ctx.db
                .op(move |tx| {
                    // Check ID exists
                    let uid = db::storage_pool::get_uid(tx, self.pool_id)?.ok_or_else(|| {
                        TypedError::value_not_found("storage pool ID", self.pool_id)
                    })?;

                    // Check all of the given target IDs exist
                    db::target::validate_ids(tx, &self.add_target_ids, NodeTypeServer::Storage)?;
                    db::target::validate_ids(tx, &self.remove_target_ids, NodeTypeServer::Storage)?;
                    db::buddy_group::validate_ids(
                        tx,
                        &self.add_buddy_group_ids,
                        NodeTypeServer::Storage,
                    )?;
                    db::buddy_group::validate_ids(
                        tx,
                        &self.remove_buddy_group_ids,
                        NodeTypeServer::Storage,
                    )?;

                    if let Some(ref new_alias) = self.alias {
                        let new_alias = &std::str::from_utf8(new_alias)?;
                        // Check alias is free
                        if db::entity::get_uid(tx, new_alias)?.is_some() {
                            bail!(TypedError::value_exists("Alias", new_alias));
                        }

                        db::entity::update_alias(tx, uid, new_alias)?;
                    }

                    // Move given target IDs to the given pool or the default pool
                    db::target::update_storage_pools(tx, self.pool_id, &self.add_target_ids)?;
                    db::target::update_storage_pools(
                        tx,
                        DEFAULT_STORAGE_POOL,
                        &self.remove_target_ids,
                    )?;

                    // Same with buddy groups
                    db::buddy_group::update_storage_pools(
                        tx,
                        self.pool_id,
                        &self.add_buddy_group_ids,
                    )?;
                    db::buddy_group::update_storage_pools(
                        tx,
                        DEFAULT_STORAGE_POOL,
                        &self.remove_buddy_group_ids,
                    )?;

                    Ok(())
                })
                .await
        }
        .await
        {
            Ok(_) => {
                log::info!("Storage pool {} modified", self.pool_id,);

                notify_nodes(
                    ctx,
                    &[NodeType::Meta, NodeType::Storage],
                    &RefreshStoragePools { ack_id: "".into() },
                )
                .await;

                ModifyStoragePoolResp {
                    result: OpsErr::SUCCESS,
                }
            }
            Err(err) => {
                log_error_chain!(err, "Modifying storage pool {} failed", self.pool_id);

                ModifyStoragePoolResp {
                    result: match err.downcast_ref() {
                        // Yes, returning OpsErr::INVAL is intended for value not found. Unlike
                        // remove_storage_pool, here this signals that pool ID is invalid
                        Some(TypedError::ValueNotFound { .. }) => OpsErr::INVAL,
                        _ => OpsErr::INTERNAL,
                    },
                }
            }
        }
    }
}

impl Handler for RemoveStoragePool {
    type Response = RemoveStoragePoolResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        let res = ctx
            .db
            .op(move |tx| {
                // Check ID exists
                db::storage_pool::get_uid(tx, self.pool_id)?
                    .ok_or_else(|| TypedError::value_not_found("storage pool ID", self.pool_id))?;

                // Check it is not the default pool
                if self.pool_id == DEFAULT_STORAGE_POOL {
                    bail!("The default pool cannot be removed");
                }

                // move all targets in this pool to the default pool
                db::target::reset_storage_pool(tx, self.pool_id)?;
                db::buddy_group::reset_storage_pool(tx, self.pool_id)?;

                db::storage_pool::delete(tx, self.pool_id)?;

                Ok(())
            })
            .await;

        match res {
            Ok(_) => {
                log::info!("Storage pool {} removed", self.pool_id,);

                notify_nodes(
                    ctx,
                    &[NodeType::Meta, NodeType::Storage],
                    &RefreshStoragePools { ack_id: "".into() },
                )
                .await;

                RemoveStoragePoolResp {
                    result: OpsErr::SUCCESS,
                }
            }
            Err(err) => {
                log_error_chain!(err, "Removing storage pool {} failed", self.pool_id);

                RemoveStoragePoolResp {
                    result: match err.downcast_ref() {
                        Some(TypedError::ValueNotFound { .. }) => OpsErr::UNKNOWN_POOL,
                        _ => OpsErr::INTERNAL,
                    },
                }
            }
        }
    }
}
