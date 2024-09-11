use super::*;
use crate::db::target::TargetCapacities;
use crate::types::ResolveEntityId;
use shared::bee_msg::target::*;
use std::time::Duration;

impl HandleWithResponse for GetTargetMappings {
    type Response = GetTargetMappingsResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Result<Self::Response> {
        let mapping = ctx
            .db
            .op(move |tx| db::target::get_with_type(tx, NodeTypeServer::Storage))
            .await?;

        let resp = GetTargetMappingsResp {
            mapping: mapping
                .into_iter()
                .map(|e| (e.target_id, e.node_id))
                .collect::<HashMap<_, _>>(),
        };

        Ok(resp)
    }
}

impl HandleWithResponse for GetTargetStates {
    type Response = GetTargetStatesResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Result<Self::Response> {
        let res = ctx
            .db
            .op(move |tx| db::target::get_with_type(tx, self.node_type.try_into()?))
            .await?;
        let mut targets = Vec::with_capacity(res.len());
        let mut reachability_states = Vec::with_capacity(res.len());
        let mut consistency_states = Vec::with_capacity(res.len());

        for e in res {
            targets.push(e.target_id);
            reachability_states.push(calc_reachability_state(
                e.last_contact,
                ctx.info.user_config.node_offline_timeout,
            ));
            consistency_states.push(e.consistency);
        }

        let resp = GetTargetStatesResp {
            targets,
            reachability_states,
            consistency_states,
        };

        Ok(resp)
    }
}

impl HandleWithResponse for RegisterTarget {
    type Response = RegisterTargetResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Result<Self::Response> {
        let ctx2 = ctx.clone();

        let (id, is_new) = ctx
            .db
            .op(move |tx| {
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
        let target_ids = self.target_ids.keys().copied().collect::<Vec<_>>();

        ctx.db
            .op(move |tx| {
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
        let node_type = self.node_type;
        ctx.db
            .op(move |tx| {
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
        // self.old_states is currently completely ignored. If something reports a non-GOOD state, I
        // see no apparent reason to that the old state matches before setting. We have the
        // authority, whatever nodes think their old state was doesn't matter.

        let changed = ctx
            .db
            .op(move |tx| {
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
        let node_type = self.node_type.try_into()?;
        let msg = self.clone();

        ctx.db
            .op(move |tx| {
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
