use super::*;
use shared::types::NodeType;

pub(super) async fn handle(
    msg: msg::SetTargetConsistencyStates,
    ctx: &Context,
    _req: &impl Request,
) -> msg::SetTargetConsistencyStatesResp {
    match async {
        let msg = msg.clone();

        ctx.db
            .op(move |tx| {
                // Check given target IDs exist
                db::target::validate_ids(tx, &msg.target_ids, msg.node_type)?;

                if msg.set_online > 0 {
                    db::node::update_last_contact_for_targets(tx, &msg.target_ids, msg.node_type)?;
                }

                db::target::update_consistency_states(
                    tx,
                    msg.target_ids.into_iter().zip(msg.states),
                    msg.node_type,
                )
            })
            .await?;

        notify_nodes(
            ctx,
            &[NodeType::Meta, NodeType::Storage, NodeType::Client],
            &msg::RefreshTargetStates { ack_id: "".into() },
        )
        .await;

        Ok(()) as Result<()>
    }
    .await
    {
        Ok(_) => {
            log::info!("Set consistency state for targets {:?}", msg.target_ids,);
            msg::SetTargetConsistencyStatesResp {
                result: OpsErr::SUCCESS,
            }
        }

        Err(err) => {
            log_error_chain!(
                err,
                "Setting consistency state for targets {:?} failed",
                msg.target_ids
            );
            msg::SetTargetConsistencyStatesResp {
                result: OpsErr::INTERNAL,
            }
        }
    }
}
