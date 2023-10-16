use super::*;
use shared::msg::refresh_capacity_pools::RefreshCapacityPools;
use shared::msg::set_mirror_buddy_group::{SetMirrorBuddyGroup, SetMirrorBuddyGroupResp};
use shared::types::NodeType;

pub(super) async fn handle(
    msg: SetMirrorBuddyGroup,
    ctx: &Context,
    _req: &impl Request,
) -> SetMirrorBuddyGroupResp {
    match ctx
        .db
        .op(move |tx| {
            // Check buddy group doesn't exist
            if db::buddy_group::get_uid(tx, msg.buddy_group_id, msg.node_type)?.is_some() {
                bail!(TypedError::value_exists(
                    "buddy group ID",
                    msg.buddy_group_id
                ));
            }

            // Check targets exist
            db::target::validate_ids(
                tx,
                &[msg.primary_target_id, msg.secondary_target_id],
                msg.node_type,
            )?;

            db::buddy_group::insert(
                tx,
                if msg.buddy_group_id == 0 {
                    None
                } else {
                    Some(msg.buddy_group_id)
                },
                msg.node_type,
                msg.primary_target_id,
                msg.secondary_target_id,
            )
        })
        .await
    {
        Ok(actual_id) => {
            log::info!(
                "Added new {:?} buddy group with ID {} (Requested: {})",
                msg.node_type,
                actual_id,
                msg.buddy_group_id,
            );

            notify_nodes(
                ctx,
                &[NodeType::Meta, NodeType::Storage, NodeType::Client],
                &SetMirrorBuddyGroup {
                    ack_id: "".into(),
                    node_type: msg.node_type,
                    primary_target_id: msg.primary_target_id,
                    secondary_target_id: msg.secondary_target_id,
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
                msg.node_type,
                msg.buddy_group_id
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
