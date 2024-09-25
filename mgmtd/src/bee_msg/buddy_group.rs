use super::target::calc_reachability_state;
use super::*;
use shared::bee_msg::buddy_group::*;

impl HandleWithResponse for GetMirrorBuddyGroups {
    type Response = GetMirrorBuddyGroupsResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Result<Self::Response> {
        let groups = ctx
            .db
            .op(move |tx| db::buddy_group::get_with_type(tx, self.node_type.try_into()?))
            .await?;

        let mut buddy_groups = vec![];
        let mut primary_targets = vec![];
        let mut secondary_targets = vec![];

        for g in groups {
            buddy_groups.push(g.id);
            primary_targets.push(g.primary_target_id);
            secondary_targets.push(g.secondary_target_id);
        }

        let resp = GetMirrorBuddyGroupsResp {
            buddy_groups,
            primary_targets,
            secondary_targets,
        };

        Ok(resp)
    }
}

impl HandleNoResponse for SetMirrorBuddyGroupResp {
    async fn handle(self, _ctx: &Context, _req: &mut impl Request) -> Result<()> {
        // response from server nodes to SetMirrorBuddyGroup notification
        Ok(())
    }
}

impl HandleWithResponse for GetStatesAndBuddyGroups {
    type Response = GetStatesAndBuddyGroupsResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Result<Self::Response> {
        let (targets, groups) = ctx
            .db
            .op(move |tx| {
                let node_type = self.node_type.try_into()?;

                let targets = db::target::get_with_type(tx, node_type)?;
                let groups = db::buddy_group::get_with_type(tx, node_type)?;

                Ok((targets, groups))
            })
            .await?;

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

        let resp = GetStatesAndBuddyGroupsResp {
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
        };

        // If it's a client that requested it, notify the run controller that it pulled states
        if self.requested_by_client_id != 0 {
            ctx.notify_client_pulled_state(self.node_type, self.requested_by_client_id);
        }

        Ok(resp)
    }
}
