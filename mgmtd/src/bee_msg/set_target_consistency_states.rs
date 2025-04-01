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

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(ctx)?;

        let node_type = self.node_type.try_into()?;
        let msg = self.clone();

        ctx.db
            .write_tx(move |tx| {
                // Check given target Ids exist
                db::target::validate_ids(tx, &msg.target_ids, node_type)?;

                if msg.set_online > 0 {
                    update_last_contact_times(tx, &msg.target_ids, node_type)?;
                }

                db::target::update_consistency_states(
                    tx,
                    msg.target_ids.into_iter().zip(msg.states.iter().copied()),
                    node_type,
                )
            })
            .await?;

        log::info!("Set consistency state for targets {:?}", self.target_ids,);

        notify_nodes(
            ctx,
            &[NodeType::Meta, NodeType::Storage, NodeType::Client],
            &RefreshTargetStates { ack_id: "".into() },
        )
        .await;

        Ok(SetTargetConsistencyStatesResp {
            result: OpsErr::SUCCESS,
        })
    }
}
