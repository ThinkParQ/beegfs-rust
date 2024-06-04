use super::target::calc_reachability_state;
use super::*;
use crate::db::misc::MetaRoot;
use shared::bee_msg::buddy_group::*;

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
        let res = ctx
            .db
            .op(move |tx| {
                let node_type = self.node_type.try_into()?;

                let targets = db::target::get_with_type(tx, node_type)?;
                let groups = db::buddy_group::get_with_type(tx, node_type)?;

                Ok((targets, groups))
            })
            .await;

        match res {
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
                                consistency: e.consistency,
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

impl Handler for SetMetadataMirroring {
    type Response = SetMetadataMirroringResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        let res = async {
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
        .await;

        match res {
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
