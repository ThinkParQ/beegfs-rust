use super::*;
use shared::bee_msg::buddy_group::*;
use target::get_targets_with_states;

impl HandleWithResponse for GetMirrorBuddyGroups {
    type Response = GetMirrorBuddyGroupsResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Result<Self::Response> {
        let groups: Vec<(BuddyGroupId, TargetId, TargetId)> = ctx
            .db
            .op(move |tx| {
                tx.query_map_collect(
                    sql!(
                        "SELECT group_id, p_target_id, s_target_id FROM all_buddy_groups_v
                        WHERE node_type = ?1"
                    ),
                    [self.node_type.sql_variant()],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(Into::into)
            })
            .await?;

        let mut buddy_groups = Vec::with_capacity(groups.len());
        let mut primary_targets = Vec::with_capacity(groups.len());
        let mut secondary_targets = Vec::with_capacity(groups.len());

        for g in groups {
            buddy_groups.push(g.0);
            primary_targets.push(g.1);
            secondary_targets.push(g.2);
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
        let node_type: NodeTypeServer = self.node_type.try_into()?;

        let pre_shutdown = ctx.run_state.pre_shutdown();
        let node_offline_timeout = ctx.info.user_config.node_offline_timeout;

        let (targets, groups) = ctx
            .db
            .op(move |tx| {
                let targets = get_targets_with_states(
                    tx,
                    pre_shutdown,
                    self.node_type.try_into()?,
                    node_offline_timeout,
                )?;

                let groups: Vec<(BuddyGroupId, TargetId, TargetId)> = tx.query_map_collect(
                    sql!(
                        "SELECT group_id, p_target_id, s_target_id FROM all_buddy_groups_v
                        WHERE node_type = ?1"
                    ),
                    [node_type.sql_variant()],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )?;

                Ok((targets, groups))
            })
            .await?;

        let states: HashMap<_, _> = targets
            .into_iter()
            .map(|e| {
                (
                    e.0,
                    CombinedTargetState {
                        reachability: e.2,
                        consistency: e.1,
                    },
                )
            })
            .collect();

        let resp = GetStatesAndBuddyGroupsResp {
            groups: groups
                .into_iter()
                .map(|e| {
                    (
                        e.0,
                        BuddyGroup {
                            primary_target_id: e.1,
                            secondary_target_id: e.2,
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
