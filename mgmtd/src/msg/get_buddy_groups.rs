use super::*;

pub(super) async fn handle(
    msg: msg::GetMirrorBuddyGroups,
    rcc: impl RequestConnectionController,
    ci: impl ComponentInteractor,
) -> Result<()> {
    match ci
        .execute_db(move |tx| db::buddy_groups::with_type(tx, msg.node_type))
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

            rcc.respond(&msg::GetMirrorBuddyGroupsResp {
                buddy_groups,
                primary_targets,
                secondary_targets,
            })
            .await
        }
        Err(err) => {
            log::error!("Getting buddy groups failed:\n{:?}", err);
            rcc.respond(&msg::GetMirrorBuddyGroupsResp {
                buddy_groups: vec![],
                primary_targets: vec![],
                secondary_targets: vec![],
            })
            .await
        }
    }
}
