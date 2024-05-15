use super::*;
use crate::db::target::TargetCapacities;
use crate::types::{NodeType, NodeTypeServer, TargetConsistencyState};
use shared::bee_msg::misc::RefreshCapacityPools;
use shared::bee_msg::target::*;
use std::time::Duration;

impl Handler for GetTargetMappings {
    type Response = GetTargetMappingsResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        match ctx
            .db
            .op(move |tx| db::target::get_with_type(tx, NodeTypeServer::Storage))
            .await
        {
            Ok(res) => GetTargetMappingsResp {
                mapping: res
                    .into_iter()
                    .map(|e| (e.target_id, e.node_id))
                    .collect::<HashMap<_, _>>(),
            },
            Err(err) => {
                log_error_chain!(err, "Getting target mappings failed");
                GetTargetMappingsResp {
                    mapping: HashMap::new(),
                }
            }
        }
    }
}

impl Handler for GetTargetStates {
    type Response = GetTargetStatesResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        match ctx
            .db
            .op(move |tx| db::target::get_with_type(tx, self.node_type.try_into()?))
            .await
        {
            Ok(res) => {
                let mut targets = Vec::with_capacity(res.len());
                let mut reachability_states = Vec::with_capacity(res.len());
                let mut consistency_states = Vec::with_capacity(res.len());

                for e in res {
                    targets.push(e.target_id);
                    reachability_states.push(calc_reachability_state(
                        e.last_contact,
                        ctx.info.user_config.node_offline_timeout,
                    ));
                    consistency_states.push(e.consistency.into());
                }

                GetTargetStatesResp {
                    targets,
                    reachability_states,
                    consistency_states,
                }
            }
            Err(err) => {
                log_error_chain!(
                    err,
                    "Getting target states for {:?} nodes failed",
                    self.node_type,
                );

                GetTargetStatesResp {
                    targets: vec![],
                    reachability_states: vec![],
                    consistency_states: vec![],
                }
            }
        }
    }
}

impl Handler for RegisterTarget {
    type Response = RegisterTargetResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        let res = async move {
            if !ctx.info.user_config.registration_enable {
                bail!("Registration of new targets is not allowed");
            }

            ctx.db
                .op(move |tx| {
                    db::target::insert_storage(
                        tx,
                        self.target_id,
                        Some(format!("target_{}", std::str::from_utf8(&self.alias)?).as_str()),
                    )
                })
                .await
        }
        .await;

        match res {
            Ok(id) => {
                log::info!("Registered storage target {id}");
                RegisterTargetResp { id }
            }
            Err(err) => {
                log_error_chain!(err, "Registering storage target {} failed", self.target_id);
                RegisterTargetResp { id: 0 }
            }
        }
    }
}

impl Handler for MapTargets {
    type Response = MapTargetsResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        let target_ids = self.target_ids.keys().copied().collect::<Vec<_>>();

        let res = ctx
            .db
            .op(move |tx| {
                // Check node ID exists
                if db::node::get_uid(tx, self.node_id, NodeType::Storage)?.is_none() {
                    bail!(TypedError::value_not_found("node ID", self.node_id));
                }

                // Check all target IDs exist
                db::target::validate_ids(tx, &target_ids, NodeTypeServer::Storage)?;

                let updated =
                    db::target::update_storage_node_mappings(tx, &target_ids, self.node_id)?;

                Ok(updated)
            })
            .await;

        match res {
            Ok(updated) => {
                log::info!(
                    "Mapped {} storage targets to node {}",
                    updated,
                    self.node_id
                );

                // TODO only do it with successful ones
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

                // Storage server expects a separate status code for each target map requested.
                // For simplicity we just do an all-or-nothing approach: If all mappings succeed, we
                // return success. If at least one fails, we fail the whole operation and send back
                // an empty result (see below). The storage handles this as errors.
                // This mechanism is supposed to go away later anyway, so this
                // solution is fine.
                MapTargetsResp {
                    results: self
                        .target_ids
                        .into_iter()
                        .map(|e| (e.0, OpsErr::SUCCESS))
                        .collect(),
                }
            }
            Err(err) => {
                log_error_chain!(err, "Mapping storage targets failed");

                MapTargetsResp {
                    results: HashMap::new(),
                }
            }
        }
    }
}

impl Handler for MapTargetsResp {
    type Response = ();

    async fn handle(self, _ctx: &Context, _req: &mut impl Request) -> Self::Response {
        // This is sent from the nodes as a result of the MapTargets notification after
        // map_targets was called. We just ignore it.
    }
}

impl Handler for UnmapTarget {
    type Response = UnmapTargetResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        let res = ctx
            .db
            .op(move |tx| {
                // Check given target ID exists
                db::target::get_uid(tx, self.target_id, NodeTypeServer::Storage)?
                    .ok_or_else(|| TypedError::value_not_found("target ID", self.target_id))?;

                db::target::delete_storage(tx, self.target_id)
            })
            .await;

        match res {
            Ok(_) => {
                log::info!("Removed storage target {}", self.target_id,);

                notify_nodes(
                    ctx,
                    &[NodeType::Meta],
                    &RefreshCapacityPools { ack_id: "".into() },
                )
                .await;

                UnmapTargetResp {
                    result: OpsErr::SUCCESS,
                }
            }
            Err(err) => {
                log_error_chain!(err, "Unmapping storage target {} failed", self.target_id);

                UnmapTargetResp {
                    result: OpsErr::INTERNAL,
                }
            }
        }
    }
}

impl Handler for SetStorageTargetInfo {
    type Response = SetStorageTargetInfoResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        let node_type = self.node_type;
        match ctx
            .db
            .op(move |tx| {
                db::target::get_and_update_capacities(
                    tx,
                    self.info.into_iter().map(|e| {
                        (
                            e.target_id,
                            TargetCapacities {
                                total_space: Some(e.total_space),
                                total_inodes: Some(e.total_inodes),
                                free_space: Some(e.free_space),
                                free_inodes: Some(e.free_inodes),
                            },
                        )
                    }),
                    self.node_type.try_into()?,
                )
            })
            .await
        {
            Ok(_) => {
                log::info!("Updated {:?} target info", node_type,);

                // in the old mgmtd, a notice to refresh cap pools is sent out here if a cap pool
                // changed I consider this being to expensive to check here and just don't
                // do it. Nodes refresh their cap pool every two minutes (by default) anyway

                SetStorageTargetInfoResp {
                    result: OpsErr::SUCCESS,
                }
            }

            Err(err) => {
                log_error_chain!(err, "Updating {:?} target info failed", node_type);
                SetStorageTargetInfoResp {
                    result: OpsErr::INTERNAL,
                }
            }
        }
    }
}

impl Handler for ChangeTargetConsistencyStates {
    type Response = ChangeTargetConsistencyStatesResp;

    async fn handle(self, ctx: &Context, __req: &mut impl Request) -> Self::Response {
        // self.old_states is currently completely ignored. If something reports a non-GOOD state, I
        // see no apparent reason to that the old state matches before setting. We have the
        // authority, whatever nodes think their old state was doesn't matter.

        let res = ctx
            .db
            .op(move |tx| {
                let node_type = self.node_type.try_into()?;

                // Check given target IDs exist
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
            .await;

        match res {
            Ok(changed) => {
                log::info!(
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

                ChangeTargetConsistencyStatesResp {
                    result: OpsErr::SUCCESS,
                }
            }
            Err(err) => {
                log_error_chain!(
                    err,
                    "Updating target consistency states for {:?} nodes failed",
                    self.node_type
                );

                ChangeTargetConsistencyStatesResp {
                    result: OpsErr::INTERNAL,
                }
            }
        }
    }
}

impl Handler for SetTargetConsistencyStates {
    type Response = SetTargetConsistencyStatesResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        let res = async {
            let node_type = self.node_type.try_into()?;
            let msg = self.clone();

            ctx.db
                .op(move |tx| {
                    // Check given target IDs exist
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

            notify_nodes(
                ctx,
                &[NodeType::Meta, NodeType::Storage, NodeType::Client],
                &RefreshTargetStates { ack_id: "".into() },
            )
            .await;

            Ok(()) as Result<()>
        }
        .await;

        match res {
            Ok(_) => {
                log::info!("Set consistency state for targets {:?}", self.target_ids,);
                SetTargetConsistencyStatesResp {
                    result: OpsErr::SUCCESS,
                }
            }

            Err(err) => {
                log_error_chain!(
                    err,
                    "Setting consistency state for targets {:?} failed",
                    self.target_ids
                );
                SetTargetConsistencyStatesResp {
                    result: OpsErr::INTERNAL,
                }
            }
        }
    }
}

/// Calculate reachability state as requested by old BeeGFS code.
pub(crate) fn calc_reachability_state(
    contact_age: Duration,
    timeout: Duration,
) -> TargetReachabilityState {
    if contact_age > timeout {
        TargetReachabilityState::Offline
    } else if contact_age > timeout / 2 {
        TargetReachabilityState::ProbablyOffline
    } else {
        TargetReachabilityState::Online
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_calc_reachability_state() {
        assert_eq!(
            TargetReachabilityState::Online,
            calc_reachability_state(Duration::from_secs(5), Duration::from_secs(60))
        );

        assert_eq!(
            TargetReachabilityState::Online,
            calc_reachability_state(Duration::from_secs(30), Duration::from_secs(60))
        );

        assert_eq!(
            TargetReachabilityState::ProbablyOffline,
            calc_reachability_state(Duration::from_secs(31), Duration::from_secs(60))
        );

        assert_eq!(
            TargetReachabilityState::ProbablyOffline,
            calc_reachability_state(Duration::from_secs(60), Duration::from_secs(60))
        );

        assert_eq!(
            TargetReachabilityState::Offline,
            calc_reachability_state(Duration::from_secs(61), Duration::from_secs(60))
        );
    }
}
