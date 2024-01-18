use super::*;
use crate::types::{EntityType, NodeType, NodeTypeServer};
use shared::beemsg::misc::CapacityPool;
use shared::beemsg::storage_pool::*;
use shared::types::{BuddyGroupID, NodeID, TargetID, DEFAULT_STORAGE_POOL};

impl Handler for AddStoragePool {
    type Response = AddStoragePoolResp;

    async fn handle(self, ctx: &Context, __req: &mut impl Request) -> Self::Response {
        match ctx
            .db
            .op(move |tx| {
                let alias = &std::str::from_utf8(&self.alias)?;

                // Check alias is free
                if db::entity::get_uid(tx, alias)?.is_some() {
                    bail!(TypedError::value_exists("Alias", alias));
                }

                // Check all of the given target IDs exist
                db::target::validate_ids(tx, &self.move_target_ids, NodeTypeServer::Storage)?;
                // Check all of the given buddy group IDs exist
                db::buddy_group::validate_ids(
                    tx,
                    &self.move_buddy_group_ids,
                    NodeTypeServer::Storage,
                )?;

                let pool_id = if self.pool_id != 0 {
                    // Check given pool_id is free
                    if db::storage_pool::get_uid(tx, self.pool_id)?.is_some() {
                        bail!(TypedError::value_exists("storage pool ID", self.pool_id));
                    }

                    self.pool_id
                } else {
                    db::misc::find_new_id(tx, "storage_pools", "pool_id", 1..=0xFFFF)?
                };

                // Insert entity then storage pool entry
                let new_uid = db::entity::insert(tx, EntityType::StoragePool, alias)?;
                db::storage_pool::insert(tx, pool_id, new_uid)?;

                // Update storage pool assignments for the given targets
                db::target::update_storage_pools(tx, pool_id, &self.move_target_ids)?;
                db::buddy_group::update_storage_pools(tx, pool_id, &self.move_buddy_group_ids)?;

                Ok(pool_id)
            })
            .await
        {
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

impl Handler for GetStoragePools {
    type Response = GetStoragePoolsResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        let pools = match async move {
            let config = &ctx.info.user_config;

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

                    // Only collect targets belonging to the current pool
                    for target in targets.iter().filter(|t| t.pool_id == pool.pool_id) {
                        let cap_pool_i = usize::from(CapacityPool::from(target.cap_pool));
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
                        buddy_group_cap_pools[usize::from(CapacityPool::from(group.cap_pool))]
                            .push(group.entity_id);
                    }

                    Ok(StoragePool {
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
                .collect::<Result<Vec<StoragePool>>>() as Result<_>
        }
        .await
        {
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
        match ctx
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
            .await
        {
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
