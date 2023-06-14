use super::*;

pub(super) async fn handle(
    msg: msg::SetTargetConsistencyStates,
    ci: impl ComponentInteractor,
    _rcc: &impl RequestConnectionController,
) -> msg::SetTargetConsistencyStatesResp {
    match async {
        let msg = msg.clone();

        ci.execute_db(move |tx| {
            // Check given target IDs exist
            db::target::check_existence(tx, &msg.target_ids, msg.node_type)?;

            if msg.set_online {
                db::node::update_last_contact_for_targets(tx, &msg.target_ids, msg.node_type)?;
            }

            db::target::update_consistency_states(
                tx,
                msg.target_ids.into_iter().zip(msg.states.into_iter()),
                msg.node_type,
            )
        })
        .await?;

        ci.notify_nodes(&msg::RefreshTargetStates { ack_id: "".into() })
            .await;

        Ok(()) as DbResult<()>
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
