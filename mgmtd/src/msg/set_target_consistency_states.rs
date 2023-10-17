use super::*;
use crate::types::TargetConsistencyState;
use shared::msg::refresh_target_states::RefreshTargetStates;
use shared::msg::set_target_consistency_states::{
    SetTargetConsistencyStates, SetTargetConsistencyStatesResp,
};

pub(super) async fn handle(
    msg: SetTargetConsistencyStates,
    ctx: &Context,
    _req: &impl Request,
) -> SetTargetConsistencyStatesResp {
    match async {
        let node_type = msg.node_type.try_into()?;
        let msg = msg.clone();

        ctx.db
            .op(move |tx| {
                // Check given target IDs exist
                db::target::validate_ids(tx, &msg.target_ids, node_type)?;

                if msg.set_online > 0 {
                    db::node::update_last_contact_for_targets(tx, &msg.target_ids, node_type)?;
                }

                db::target::update_consistency_states(
                    tx,
                    msg.target_ids
                        .into_iter()
                        .zip(msg.states.iter().copied().map(TargetConsistencyState::from)),
                    node_type,
                )
            })
            .await?;

        notify_nodes(
            ctx,
            &[NodeType::Meta, NodeType::Storage, NodeType::Client],
            &RefreshTargetStates { ack_id: "".into() },
        )
        .await;

        Ok(()) as Result<()>
    }
    .await
    {
        Ok(_) => {
            log::info!("Set consistency state for targets {:?}", msg.target_ids,);
            SetTargetConsistencyStatesResp {
                result: OpsErr::SUCCESS,
            }
        }

        Err(err) => {
            log_error_chain!(
                err,
                "Setting consistency state for targets {:?} failed",
                msg.target_ids
            );
            SetTargetConsistencyStatesResp {
                result: OpsErr::INTERNAL,
            }
        }
    }
}
