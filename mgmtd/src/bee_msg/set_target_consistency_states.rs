use super::*;
use common::update_last_contact_times;
use shared::bee_msg::target::*;

impl HandleWithResponse for SetTargetConsistencyStates {
    type Response = SetTargetConsistencyStatesResp;

    fn error_response() -> Self::Response {
        SetTargetConsistencyStatesResp {
            result: OpsErr::INTERNAL,
        }
    }

    async fn handle(self, app: &impl App, _req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(app)?;

        let node_type = self.node_type.try_into()?;
        let msg = self.clone();
        let node_offline_timeout = app.static_info().user_config.node_offline_timeout;

        app.write_tx(move |tx| {
            // Check given target Ids exist
            db::target::validate_ids(tx, &msg.target_ids, node_type)?;

            if msg.set_online > 0 {
                update_last_contact_times(tx, &msg.target_ids, node_type, node_offline_timeout)?;
            }

            db::target::update_consistency_states(
                tx,
                msg.target_ids.into_iter().zip(msg.states.iter().copied()),
                node_type,
            )
        })
        .await?;

        log::info!("Set consistency state for targets {:?}", self.target_ids,);

        app.send_notifications(
            &[NodeType::Meta, NodeType::Storage, NodeType::Client],
            &RefreshTargetStates { ack_id: "".into() },
        )
        .await;

        Ok(SetTargetConsistencyStatesResp {
            result: OpsErr::SUCCESS,
        })
    }
}
