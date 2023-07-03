use super::*;

pub(super) async fn handle(
    msg: msg::ChangeTargetConsistencyStates,
    ci: impl ComponentInteractor,
    _rcc: &impl RequestConnectionController,
) -> msg::ChangeTargetConsistencyStatesResp {
    match ci
        .execute_db(move |tx| {
            // TODO This is where old mgmtd updates the "last_seen" time
            // (as this msg comes in every 30 seconds)
            // We adapt this for now, but actually want to do this independent from the msg
            // type

            // Check given target IDs exist
            db::target::check_existence(tx, &msg.target_ids, msg.node_type)?;

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
                "Updated target consistency states for {} nodes",
                msg.node_type
            );

            if changed {
                ci.notify_nodes(&msg::RefreshTargetStates { ack_id: "".into() })
                    .await;
            }

            msg::ChangeTargetConsistencyStatesResp {
                result: OpsErr::SUCCESS,
            }
        }
        Err(err) => {
            log_error_chain!(
                err,
                "Updating target consistency states for {} nodes failed",
                msg.node_type
            );

            msg::ChangeTargetConsistencyStatesResp {
                result: OpsErr::INTERNAL,
            }
        }
    }
}
