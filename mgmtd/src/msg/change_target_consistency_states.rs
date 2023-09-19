use super::*;
use shared::types::NodeType;

pub(super) async fn handle(
    msg: msg::ChangeTargetConsistencyStates,
    ctx: &Context,
    _req: &impl Request,
) -> msg::ChangeTargetConsistencyStatesResp {
    // msg.old_states is currently completely ignored. If something reports a non-GOOD state, I see
    // no apparent reason to that the old state matches before setting. We have the authority,
    // whatever nodes think their old state was doesn't matter.

    match ctx
        .db
        .op(move |tx| {
            // Check given target IDs exist
            db::target::validate_ids(tx, &msg.target_ids, msg.node_type)?;

            // Old management updates contact time while handling this message (comes usually in
            // every 30 seconds), so we do it as well
            db::node::update_last_contact_for_targets(tx, &msg.target_ids, msg.node_type)?;

            let affected = db::target::update_consistency_states(
                tx,
                msg.target_ids.into_iter().zip(msg.new_states),
                msg.node_type,
            )?;

            Ok(affected > 0)
        })
        .await
    {
        Ok(changed) => {
            log::info!(
                "Updated target consistency states for {:?} nodes",
                msg.node_type
            );

            if changed {
                notify_nodes(
                    ctx,
                    &[NodeType::Meta, NodeType::Storage, NodeType::Client],
                    &msg::RefreshTargetStates { ack_id: "".into() },
                )
                .await;
            }

            msg::ChangeTargetConsistencyStatesResp {
                result: OpsErr::SUCCESS,
            }
        }
        Err(err) => {
            log_error_chain!(
                err,
                "Updating target consistency states for {:?} nodes failed",
                msg.node_type
            );

            msg::ChangeTargetConsistencyStatesResp {
                result: OpsErr::INTERNAL,
            }
        }
    }
}
