use super::*;

pub(super) async fn handle(
    msg: msg::SetTargetConsistencyStates,
    rcc: impl RequestConnectionController,
    ci: impl ComponentInteractor,
) -> Result<()> {
    match async {
        let msg = msg.clone();

        ci.execute_db(move |tx| {
            if msg.set_online {
                db::nodes::update_last_contact_for_targets(
                    tx,
                    msg.targets.iter().copied(),
                    msg.node_type.into(),
                )?;
            }

            db::targets::update_consistency_states(
                tx,
                msg.targets.into_iter().zip(msg.states.into_iter()),
                msg.node_type,
            )
        })
        .await?;

        ci.notify_nodes(&msg::RefreshTargetStates { ack_id: "".into() })
            .await;

        Ok(()) as Result<()>
    }
    .await
    {
        Ok(_) => {
            log::info!("Set consistency state for targets {:?}", msg.targets,);
            rcc.respond(&msg::SetTargetConsistencyStatesResp {
                result: OpsErr::SUCCESS,
            })
            .await
        }

        Err(err) => {
            log::error!(
                "Setting consistency state for targets {:?} failed:\n{:?}",
                msg.targets,
                err
            );
            rcc.respond(&msg::SetTargetConsistencyStatesResp {
                result: OpsErr::INTERNAL,
            })
            .await
        }
    }
}
