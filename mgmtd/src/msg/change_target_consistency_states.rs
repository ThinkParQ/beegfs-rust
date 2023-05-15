use super::*;

pub(super) async fn handle(
    msg: msg::ChangeTargetConsistencyStates,
    rcc: impl RequestConnectionController,
    ci: impl ComponentInteractor,
) -> Result<()> {
    match ci
        .execute_db(move |tx| {
            // TODO This is where old mgmtd updates the "last_seen" time
            // (as this msg comes in every 30 seconds)
            // We adapt this for now, but actually want to do this independent from the msg
            // type
            db::nodes::update_last_contact_for_targets(
                tx,
                msg.target_ids.iter().copied(),
                msg.node_type.into(),
            )?;

            let affected = db::targets::update_consistency_states(
                tx,
                msg.target_ids.into_iter().zip(msg.new_states.into_iter()),
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

            rcc.respond(&msg::ChangeTargetConsistencyStatesResp {
                result: OpsErr::SUCCESS,
            })
            .await
        }
        Err(err) => {
            log::error!(
                "Updating target consistency states for {} nodes failed:\n{:?}",
                msg.node_type,
                err
            );

            rcc.respond(&msg::ChangeTargetConsistencyStatesResp {
                result: OpsErr::INTERNAL,
            })
            .await
        }
    }
}
