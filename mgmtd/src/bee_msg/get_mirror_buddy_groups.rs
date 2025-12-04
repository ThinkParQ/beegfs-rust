use super::*;
use shared::bee_msg::buddy_group::*;

impl HandleWithResponse for GetMirrorBuddyGroups {
    type Response = GetMirrorBuddyGroupsResp;

    async fn handle(self, app: &impl App, _req: &mut impl Request) -> Result<Self::Response> {
        let groups: Vec<(BuddyGroupId, TargetId, TargetId)> = app
            .read_tx(move |tx| {
                tx.query_map_collect(
                    sql!(
                        "SELECT group_id, p_target_id, s_target_id FROM buddy_groups_ext
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
