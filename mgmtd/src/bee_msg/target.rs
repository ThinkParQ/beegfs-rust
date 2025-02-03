use super::*;
use crate::db::target::TargetCapacities;
use crate::types::ResolveEntityId;
use rusqlite::Transaction;
use shared::bee_msg::target::*;
use std::time::Duration;

impl HandleWithResponse for GetTargetMappings {
    type Response = GetTargetMappingsResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Result<Self::Response> {
        let mapping: HashMap<TargetId, NodeId> = ctx
            .db
            .read_tx(move |tx| {
                tx.query_map_collect(
                    sql!(
                        "SELECT target_id, node_id
                        FROM storage_targets
                        WHERE node_id IS NOT NULL"
                    ),
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(Into::into)
            })
            .await?;

        let resp = GetTargetMappingsResp { mapping };

        Ok(resp)
    }
}

impl HandleWithResponse for GetTargetStates {
    type Response = GetTargetStatesResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Result<Self::Response> {
        let pre_shutdown = ctx.run_state.pre_shutdown();
        let node_offline_timeout = ctx.info.user_config.node_offline_timeout;

        let targets = ctx
            .db
            .read_tx(move |tx| {
                get_targets_with_states(
                    tx,
                    pre_shutdown,
                    self.node_type.try_into()?,
                    node_offline_timeout,
                )
            })
            .await?;

        let mut target_ids = Vec::with_capacity(targets.len());
        let mut reachability_states = Vec::with_capacity(targets.len());
        let mut consistency_states = Vec::with_capacity(targets.len());

        for e in targets {
            target_ids.push(e.0);
            consistency_states.push(e.1);
            reachability_states.push(e.2);
        }

        let resp = GetTargetStatesResp {
            targets: target_ids,
            consistency_states,
            reachability_states,
        };

        Ok(resp)
    }
}

pub(crate) fn get_targets_with_states(
    tx: &Transaction,
    pre_shutdown: bool,
    node_type: NodeTypeServer,
    node_offline_timeout: Duration,
) -> Result<Vec<(TargetId, TargetConsistencyState, TargetReachabilityState)>> {
    let targets = tx.query_map_collect(
        sql!(
            "SELECT t.target_id, t.consistency,
                (UNIXEPOCH('now') - UNIXEPOCH(n.last_contact)), gp.p_target_id, gs.s_target_id
            FROM targets AS t
            INNER JOIN nodes AS n USING(node_type, node_id)
            LEFT JOIN buddy_groups AS gp ON gp.p_target_id = t.target_id AND gp.node_type = t.node_type
            LEFT JOIN buddy_groups AS gs ON gs.s_target_id = t.target_id AND gs.node_type = t.node_type
            WHERE t.node_type = ?1"
        ),
        [node_type.sql_variant()],
        |row| {
            let is_primary = row.get::<_, Option<TargetId>>(3)?.is_some();
            let is_secondary = row.get::<_, Option<TargetId>>(4)?.is_some();

            Ok((
                row.get(0)?,
                TargetConsistencyState::from_row(row, 1)?,
                if !pre_shutdown || is_secondary {
                    let age = Duration::from_secs(row.get(2)?);

                    // We never want to report a primary node of a buddy group as offline since this
                    // is considered invalid. Instead we just report ProbablyOffline and wait for the switchover.
                    if !is_primary && age > node_offline_timeout {
                        TargetReachabilityState::Offline
                    } else if age > node_offline_timeout / 2 {
                        TargetReachabilityState::ProbablyOffline
                    } else {
                        TargetReachabilityState::Online
                    }
                } else {
                    TargetReachabilityState::ProbablyOffline
                },
            ))
        },
    )?;

    Ok(targets)
}

impl HandleWithResponse for RegisterTarget {
    type Response = RegisterTargetResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(ctx)?;

        let ctx2 = ctx.clone();

        let (id, is_new) = ctx
            .db
            .write_tx(move |tx| {
                // Do not do anything if the target already exists
                if let Some(id) = try_resolve_num_id(
                    tx,
                    EntityType::Target,
                    NodeType::Storage,
                    self.target_id.into(),
                )? {
                    return Ok((id.num_id().try_into()?, false));
                }

                if ctx2.info.user_config.registration_disable {
                    bail!("Registration of new targets is not allowed");
                }

                Ok((
                    db::target::insert_storage(
                        tx,
                        self.target_id,
                        Some(format!("target_{}", std::str::from_utf8(&self.alias)?).try_into()?),
                    )?,
                    true,
                ))
            })
            .await?;

        if is_new {
            log::info!("Registered new storage target with Id {id}");
        } else {
            log::debug!("Re-registered existing storage target with Id {id}");
        }

        Ok(RegisterTargetResp { id })
    }
}

impl HandleWithResponse for MapTargets {
    type Response = MapTargetsResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(ctx)?;

        let target_ids = self.target_ids.keys().copied().collect::<Vec<_>>();

        ctx.db
            .write_tx(move |tx| {
                // Check node Id exists
                let node = LegacyId {
                    node_type: NodeType::Storage,
                    num_id: self.node_id,
                }
                .resolve(tx, EntityType::Node)?;
                // Check all target Ids exist
                db::target::validate_ids(tx, &target_ids, NodeTypeServer::Storage)?;
                // Due to the check above, this must always match all the given ids
                db::target::update_storage_node_mappings(tx, &target_ids, node.num_id())?;
                Ok(())
            })
            .await?;

        // At this point, all mappings must have been successful

        log::info!(
            "Mapped storage targets with Ids {:?} to node {}",
            self.target_ids.keys(),
            self.node_id
        );

        notify_nodes(
            ctx,
            &[NodeType::Meta, NodeType::Storage, NodeType::Client],
            &MapTargets {
                target_ids: self.target_ids.clone(),
                node_id: self.node_id,
                ack_id: "".into(),
            },
        )
        .await;

        // Storage server expects a separate status code for each target map requested. We, however,
        // do a all-or-nothing approach. If e.g. one target id doesn't exist (which is an
        // exceptional error and should usually not happen anyway), we fail the whole
        // operation. Therefore we can just send a list of successes.
        let resp = MapTargetsResp {
            results: self
                .target_ids
                .into_iter()
                .map(|e| (e.0, OpsErr::SUCCESS))
                .collect(),
        };

        Ok(resp)
    }
}

impl HandleNoResponse for MapTargetsResp {
    async fn handle(self, _ctx: &Context, _req: &mut impl Request) -> Result<()> {
        // This is sent from the nodes as a result of the MapTargets notification after
        // map_targets was called. We just ignore it.
        Ok(())
    }
}

impl HandleWithResponse for SetStorageTargetInfo {
    type Response = SetStorageTargetInfoResp;

    fn error_response() -> Self::Response {
        SetStorageTargetInfoResp {
            result: OpsErr::INTERNAL,
        }
    }

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(ctx)?;

        let node_type = self.node_type;
        ctx.db
            .write_tx(move |tx| {
                db::target::get_and_update_capacities(
                    tx,
                    self.info.into_iter().map(|e| {
                        Ok((
                            e.target_id,
                            TargetCapacities {
                                total_space: Some(e.total_space.try_into()?),
                                total_inodes: Some(e.total_inodes.try_into()?),
                                free_space: Some(e.free_space.try_into()?),
                                free_inodes: Some(e.free_inodes.try_into()?),
                            },
                        ))
                    }),
                    self.node_type.try_into()?,
                )
            })
            .await?;

        log::debug!("Updated {:?} target info", node_type,);

        // in the old mgmtd, a notice to refresh cap pools is sent out here if a cap pool
        // changed I consider this being to expensive to check here and just don't
        // do it. Nodes refresh their cap pool every two minutes (by default) anyway

        Ok(SetStorageTargetInfoResp {
            result: OpsErr::SUCCESS,
        })
    }
}

impl HandleWithResponse for ChangeTargetConsistencyStates {
    type Response = ChangeTargetConsistencyStatesResp;

    fn error_response() -> Self::Response {
        ChangeTargetConsistencyStatesResp {
            result: OpsErr::INTERNAL,
        }
    }

    async fn handle(self, ctx: &Context, __req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(ctx)?;

        // self.old_states is currently completely ignored. If something reports a non-GOOD state, I
        // see no apparent reason to that the old state matches before setting. We have the
        // authority, whatever nodes think their old state was doesn't matter.

        let changed = ctx
            .db
            .write_tx(move |tx| {
                let node_type = self.node_type.try_into()?;

                // Check given target Ids exist
                db::target::validate_ids(tx, &self.target_ids, node_type)?;

                // Old management updates contact time while handling this message (comes usually in
                // every 30 seconds), so we do it as well
                db::node::update_last_contact_for_targets(tx, &self.target_ids, node_type)?;

                let affected = db::target::update_consistency_states(
                    tx,
                    self.target_ids.into_iter().zip(
                        self.new_states
                            .iter()
                            .copied()
                            .map(TargetConsistencyState::from),
                    ),
                    node_type,
                )?;

                Ok(affected > 0)
            })
            .await?;

        log::debug!(
            "Updated target consistency states for {:?} nodes",
            self.node_type
        );

        if changed {
            notify_nodes(
                ctx,
                &[NodeType::Meta, NodeType::Storage, NodeType::Client],
                &RefreshTargetStates { ack_id: "".into() },
            )
            .await;
        }

        Ok(ChangeTargetConsistencyStatesResp {
            result: OpsErr::SUCCESS,
        })
    }
}

impl HandleWithResponse for SetTargetConsistencyStates {
    type Response = SetTargetConsistencyStatesResp;

    fn error_response() -> Self::Response {
        SetTargetConsistencyStatesResp {
            result: OpsErr::INTERNAL,
        }
    }

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(ctx)?;

        let node_type = self.node_type.try_into()?;
        let msg = self.clone();

        ctx.db
            .write_tx(move |tx| {
                // Check given target Ids exist
                db::target::validate_ids(tx, &msg.target_ids, node_type)?;

                if msg.set_online > 0 {
                    db::node::update_last_contact_for_targets(tx, &msg.target_ids, node_type)?;
                }

                db::target::update_consistency_states(
                    tx,
                    msg.target_ids
                        .into_iter()
                        .zip(msg.states.iter().copied().map(TargetConsistencyState::from)),
                    node_type,
                )
            })
            .await?;

        log::info!("Set consistency state for targets {:?}", self.target_ids,);

        notify_nodes(
            ctx,
            &[NodeType::Meta, NodeType::Storage, NodeType::Client],
            &RefreshTargetStates { ack_id: "".into() },
        )
        .await;

        Ok(SetTargetConsistencyStatesResp {
            result: OpsErr::SUCCESS,
        })
    }
}
