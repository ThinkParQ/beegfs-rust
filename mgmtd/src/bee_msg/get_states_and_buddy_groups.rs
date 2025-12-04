use super::*;
use common::get_targets_with_states;
use shared::bee_msg::buddy_group::*;

impl HandleWithResponse for GetStatesAndBuddyGroups {
    type Response = GetStatesAndBuddyGroupsResp;

    async fn handle(self, app: &impl App, _req: &mut impl Request) -> Result<Self::Response> {
        let node_type: NodeTypeServer = self.node_type.try_into()?;

        let pre_shutdown = app.is_pre_shutdown();
        let node_offline_timeout = app.static_info().user_config.node_offline_timeout;

        let (targets, groups) = app
            .read_tx(move |tx| {
                let targets = get_targets_with_states(
                    tx,
                    pre_shutdown,
                    self.node_type.try_into()?,
                    node_offline_timeout,
                )?;

                let groups: Vec<(BuddyGroupId, TargetId, TargetId)> = tx.query_map_collect(
                    sql!(
                        "SELECT group_id, p_target_id, s_target_id FROM buddy_groups_ext
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
            app.notify_client_pulled_state(self.node_type, self.requested_by_client_id);
        }

        Ok(resp)
    }
}
