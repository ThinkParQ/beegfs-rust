use super::*;
use crate::types::TargetConsistencyState;
use shared::msg::change_target_consistency_states::*;
use shared::msg::refresh_target_states::RefreshTargetStates;

pub(super) async fn handle(
    msg: ChangeTargetConsistencyStates,
    ctx: &Context,
    _req: &impl Request,
) -> ChangeTargetConsistencyStatesResp {
    // msg.old_states is currently completely ignored. If something reports a non-GOOD state, I see
    // no apparent reason to that the old state matches before setting. We have the authority,
    // whatever nodes think their old state was doesn't matter.

    match ctx
        .db
        .op(move |tx| {
            let node_type = msg.node_type.try_into()?;

            // Check given target IDs exist
            db::target::validate_ids(tx, &msg.target_ids, node_type)?;

            // Old management updates contact time while handling this message (comes usually in
            // every 30 seconds), so we do it as well
            db::node::update_last_contact_for_targets(tx, &msg.target_ids, node_type)?;

            let affected = db::target::update_consistency_states(
                tx,
                msg.target_ids.into_iter().zip(
                    msg.new_states
                        .iter()
                        .copied()
                        .map(TargetConsistencyState::from),
                ),
                node_type,
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
                    &RefreshTargetStates { ack_id: "".into() },
                )
                .await;
            }

            ChangeTargetConsistencyStatesResp {
                result: OpsErr::SUCCESS,
            }
        }
        Err(err) => {
            log_error_chain!(
                err,
                "Updating target consistency states for {:?} nodes failed",
                msg.node_type
            );

            ChangeTargetConsistencyStatesResp {
                result: OpsErr::INTERNAL,
            }
        }
    }
}
