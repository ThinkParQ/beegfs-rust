use super::*;

pub(super) async fn handle(
    msg: msg::SetMirrorBuddyGroup,
    ctx: &impl AppContext,
    _req: &impl Request,
) -> msg::SetMirrorBuddyGroupResp {
    match ctx
        .db_op(move |tx| {
            // Check buddy group doesn't exist
            if db::buddy_group::get_uid(tx, msg.buddy_group_id, msg.node_type)?.is_some() {
                return Err(DbError::value_exists("buddy group ID", msg.buddy_group_id));
            }

            // Check targets exist
            db::target::validate_ids(
                tx,
                &[msg.primary_target_id, msg.secondary_target_id],
                msg.node_type,
            )?;

            db::buddy_group::insert(
                tx,
                if msg.buddy_group_id == BuddyGroupID::ZERO {
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
                "Added new {} buddy group with ID {} (Requested: {})",
                msg.node_type,
                actual_id,
                msg.buddy_group_id,
            );

            ctx.notify_nodes(
                &[NodeType::Meta, NodeType::Storage, NodeType::Client],
                &msg::SetMirrorBuddyGroup {
                    ack_id: "".into(),
                    node_type: msg.node_type,
                    primary_target_id: msg.primary_target_id,
                    secondary_target_id: msg.secondary_target_id,
                    buddy_group_id: actual_id,
                    allow_update: 0,
                },
            )
            .await;

            ctx.notify_nodes(
                &[NodeType::Meta],
                &msg::RefreshCapacityPools { ack_id: "".into() },
            )
            .await;

            msg::SetMirrorBuddyGroupResp {
                result: OpsErr::SUCCESS,
                buddy_group_id: actual_id,
            }
        }
        Err(err) => {
            log_error_chain!(
                err,
                "Adding {} buddy group with ID {} failed",
                msg.node_type,
                msg.buddy_group_id
            );

            msg::SetMirrorBuddyGroupResp {
                result: match err {
                    DbError::ValueNotFound { .. } => OpsErr::UNKNOWN_TARGET,
                    DbError::ValueExists { .. } => OpsErr::EXISTS,
                    _ => OpsErr::INTERNAL,
                },
                buddy_group_id: BuddyGroupID::ZERO,
            }
        }
    }
}
