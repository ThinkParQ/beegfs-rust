use super::*;

pub(super) async fn handle(
    msg: msg::GetMirrorBuddyGroups,
    ctx: &impl AppContext,
    _req: &impl Request,
) -> msg::GetMirrorBuddyGroupsResp {
    match ctx
        .db_op(move |tx| db::buddy_group::get_with_type(tx, msg.node_type))
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

            msg::GetMirrorBuddyGroupsResp {
                buddy_groups,
                primary_targets,
                secondary_targets,
            }
        }
        Err(err) => {
            log_error_chain!(err, "Getting buddy groups failed");
            msg::GetMirrorBuddyGroupsResp {
                buddy_groups: vec![],
                primary_targets: vec![],
                secondary_targets: vec![],
            }
        }
    }
}
