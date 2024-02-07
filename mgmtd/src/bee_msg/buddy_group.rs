use super::target::calc_reachability_state;
use super::*;
use crate::db::misc::MetaRoot;
use shared::bee_msg::buddy_group::*;
use shared::bee_msg::misc::RefreshCapacityPools;

impl Handler for GetMirrorBuddyGroups {
    type Response = GetMirrorBuddyGroupsResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        match ctx
            .db
            .op(move |tx| db::buddy_group::get_with_type(tx, self.node_type.try_into()?))
            .await
        {
            Ok(groups) => {
                let mut buddy_groups = vec![];
                let mut primary_targets = vec![];
                let mut secondary_targets = vec![];

                for g in groups {
                    buddy_groups.push(g.id);
                    primary_targets.push(g.primary_target_id);
                    secondary_targets.push(g.secondary_target_id);
                }

                GetMirrorBuddyGroupsResp {
                    buddy_groups,
                    primary_targets,
                    secondary_targets,
                }
            }
            Err(err) => {
                log_error_chain!(err, "Getting buddy groups failed");
                GetMirrorBuddyGroupsResp {
                    buddy_groups: vec![],
                    primary_targets: vec![],
                    secondary_targets: vec![],
                }
            }
        }
    }
}

impl Handler for SetMirrorBuddyGroupResp {
    type Response = ();

    async fn handle(self, _ctx: &Context, _req: &mut impl Request) -> Self::Response {
        // response from server nodes to SetMirrorBuddyGroup notification
    }
}

impl Handler for GetStatesAndBuddyGroups {
    type Response = GetStatesAndBuddyGroupsResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        match ctx
            .db
            .op(move |tx| {
                let node_type = self.node_type.try_into()?;

                let targets = db::target::get_with_type(tx, node_type)?;
                let groups = db::buddy_group::get_with_type(tx, node_type)?;

                Ok((targets, groups))
            })
            .await
        {
            Ok((targets, groups)) => {
                let states: HashMap<_, _> = targets
                    .into_iter()
                    .map(|e| {
                        (
                            e.target_id,
                            CombinedTargetState {
                                reachability: calc_reachability_state(
                                    e.last_contact,
                                    ctx.info.user_config.node_offline_timeout,
                                ),
                                consistency: e.consistency.into(),
                            },
                        )
                    })
                    .collect();

                GetStatesAndBuddyGroupsResp {
                    groups: groups
                        .into_iter()
                        .map(|e| {
                            (
                                e.id,
                                BuddyGroup {
                                    primary_target_id: e.primary_target_id,
                                    secondary_target_id: e.secondary_target_id,
                                },
                            )
                        })
                        .collect(),
                    states,
                }
            }
            Err(err) => {
                log_error_chain!(
                    err,
                    "Getting states and buddy groups for {:?} nodes failed",
                    self.node_type,
                );

                GetStatesAndBuddyGroupsResp {
                    groups: HashMap::new(),
                    states: HashMap::new(),
                }
            }
        }
    }
}

impl Handler for RemoveBuddyGroup {
    type Response = RemoveBuddyGroupResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        match async {
            let node_type: NodeTypeServer = self.node_type.try_into()?;

            if node_type != NodeTypeServer::Storage {
                bail!("Can only remove storage buddy groups");
            }

            let node_ids = ctx
                .db
                .op(move |tx| {
                    db::buddy_group::validate_ids(
                        tx,
                        &[self.buddy_group_id],
                        NodeTypeServer::Storage,
                    )?;

                    db::buddy_group::prepare_storage_deletion(tx, self.buddy_group_id)
                })
                .await?;

            let res_primary: RemoveBuddyGroupResp = ctx.conn.request(node_ids.0, &self).await?;
            let res_secondary: RemoveBuddyGroupResp = ctx.conn.request(node_ids.1, &self).await?;

            if res_primary.result != OpsErr::SUCCESS || res_secondary.result != OpsErr::SUCCESS {
                bail!(
                    "Removing storage buddy group on primary and/or secondary storage node failed.
                Primary result: {:?}, Secondary result: {:?}",
                    res_primary.result,
                    res_secondary.result
                );
            }

            ctx.db
                .op(move |tx| db::buddy_group::delete_storage(tx, self.buddy_group_id))
                .await?;

            Ok(())
        }
        .await
        {
            Ok(_) => RemoveBuddyGroupResp {
                result: OpsErr::SUCCESS,
            },
            Err(err) => {
                log_error_chain!(
                    err,
                    "Removing {:?} buddy group {} failed",
                    self.node_type,
                    self.buddy_group_id
                );

                RemoveBuddyGroupResp {
                    result: match err.downcast_ref::<TypedError>() {
                        Some(TypedError::ValueNotFound { .. }) => OpsErr::UNKNOWN_TARGET,
                        Some(_) | None => OpsErr::INTERNAL,
                    },
                }
            }
        }
    }
}

impl Handler for SetMetadataMirroring {
    type Response = SetMetadataMirroringResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        match async {
            match ctx.db.op(db::misc::get_meta_root).await? {
                MetaRoot::Normal(_, node_uid) => {
                    let _: SetMetadataMirroringResp = ctx.conn.request(node_uid, &self).await?;

                    ctx.db.op(db::misc::enable_metadata_mirroring).await?;
                }
                MetaRoot::Unknown => bail!("Root inode unknown"),
                MetaRoot::Mirrored(_) => bail!("Root inode is already mirrored"),
            }

            Ok(()) as Result<()>
        }
        .await
        {
            Ok(_) => {
                log::info!("Enabled metadata mirroring");

                SetMetadataMirroringResp {
                    result: OpsErr::SUCCESS,
                }
            }
            Err(err) => {
                log_error_chain!(err, "Enabling metadata mirroring failed");

                SetMetadataMirroringResp {
                    result: OpsErr::INTERNAL,
                }
            }
        }
    }
}

impl Handler for SetMirrorBuddyGroup {
    type Response = SetMirrorBuddyGroupResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        match ctx
            .db
            .op(move |tx| {
                let node_type = self.node_type.try_into()?;

                // Check buddy group doesn't exist
                if db::buddy_group::get_uid(tx, self.buddy_group_id, node_type)?.is_some() {
                    bail!(TypedError::value_exists(
                        "buddy group ID",
                        self.buddy_group_id
                    ));
                }

                // Check targets exist
                db::target::validate_ids(
                    tx,
                    &[self.primary_target_id, self.secondary_target_id],
                    node_type,
                )?;

                db::buddy_group::insert(
                    tx,
                    if self.buddy_group_id == 0 {
                        None
                    } else {
                        Some(self.buddy_group_id)
                    },
                    node_type,
                    self.primary_target_id,
                    self.secondary_target_id,
                )
            })
            .await
        {
            Ok(actual_id) => {
                log::info!(
                    "Added new {:?} buddy group with ID {} (Requested: {})",
                    self.node_type,
                    actual_id,
                    self.buddy_group_id,
                );

                notify_nodes(
                    ctx,
                    &[NodeType::Meta, NodeType::Storage, NodeType::Client],
                    &SetMirrorBuddyGroup {
                        ack_id: "".into(),
                        node_type: self.node_type,
                        primary_target_id: self.primary_target_id,
                        secondary_target_id: self.secondary_target_id,
                        buddy_group_id: actual_id,
                        allow_update: 0,
                    },
                )
                .await;

                notify_nodes(
                    ctx,
                    &[NodeType::Meta],
                    &RefreshCapacityPools { ack_id: "".into() },
                )
                .await;

                SetMirrorBuddyGroupResp {
                    result: OpsErr::SUCCESS,
                    buddy_group_id: actual_id,
                }
            }
            Err(err) => {
                log_error_chain!(
                    err,
                    "Adding {:?} buddy group with ID {} failed",
                    self.node_type,
                    self.buddy_group_id
                );

                SetMirrorBuddyGroupResp {
                    result: match err.downcast_ref() {
                        Some(TypedError::ValueNotFound { .. }) => OpsErr::UNKNOWN_TARGET,
                        Some(TypedError::ValueExists { .. }) => OpsErr::EXISTS,
                        _ => OpsErr::INTERNAL,
                    },
                    buddy_group_id: 0,
                }
            }
        }
    }
}
