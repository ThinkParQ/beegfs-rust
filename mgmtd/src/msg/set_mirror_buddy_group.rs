use super::*;

pub(super) async fn handle(
    msg: msg::SetMirrorBuddyGroup,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    match hnd
        .execute_db(move |tx| {
            db::buddy_groups::insert(
                tx,
                if msg.buddy_group_id == BuddyGroupID::ZERO {
                    None
                } else {
                    Some(msg.buddy_group_id)
                },
                msg.node_type,
                msg.primary_target,
                msg.secondary_target,
            )
        })
        .await
    {
        Ok(actual_id) => {
            log::info!(
                "Added new {} buddy group with ID {} (Requested: {})",
                msg.node_type,
                actual_id,
                msg.buddy_group_id,
            );

            hnd.notify_nodes(&msg::SetMirrorBuddyGroup {
                ack_id: "".into(),
                node_type: msg.node_type,
                primary_target: msg.primary_target,
                secondary_target: msg.secondary_target,
                buddy_group_id: actual_id,
                allow_update: false,
            })
            .await;

            hnd.notify_nodes(&msg::RefreshCapacityPools { ack_id: "".into() })
                .await;

            chn.respond(&msg::SetMirrorBuddyGroupResp {
                result: OpsErr::SUCCESS,
                buddy_group_id: actual_id,
            })
            .await
        }
        Err(err) => {
            log::error!(
                "Adding {} buddy group with ID {} failed:\n{:?}",
                msg.node_type,
                msg.buddy_group_id,
                err
            );

            chn.respond(&msg::SetMirrorBuddyGroupResp {
                result: OpsErr::INTERNAL,
                buddy_group_id: BuddyGroupID::ZERO,
            })
            .await
        }
    }
}
