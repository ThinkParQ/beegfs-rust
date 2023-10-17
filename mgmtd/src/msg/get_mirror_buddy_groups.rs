use super::*;
use shared::msg::get_mirror_buddy_groups::{GetMirrorBuddyGroups, GetMirrorBuddyGroupsResp};

pub(super) async fn handle(
    msg: GetMirrorBuddyGroups,
    ctx: &Context,
    _req: &impl Request,
) -> GetMirrorBuddyGroupsResp {
    match ctx
        .db
        .op(move |tx| db::buddy_group::get_with_type(tx, msg.node_type.try_into()?))
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
