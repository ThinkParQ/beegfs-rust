use super::*;
use common::update_last_contact_times;
use shared::bee_msg::target::*;

impl HandleWithResponse for ChangeTargetConsistencyStates {
    type Response = ChangeTargetConsistencyStatesResp;

    fn error_response() -> Self::Response {
        ChangeTargetConsistencyStatesResp {
            result: OpsErr::INTERNAL,
        }
    }

    async fn handle(self, ctx: &Context, __req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(ctx)?;

        // self.old_states is currently completely ignored. If something reports a non-GOOD state, I
        // see no apparent reason to that the old state matches before setting. We have the
        // authority, whatever nodes think their old state was doesn't matter.

        let changed = ctx
            .db
            .write_tx(move |tx| {
                let node_type = self.node_type.try_into()?;

                // Check given target Ids exist
                db::target::validate_ids(tx, &self.target_ids, node_type)?;

                // Old management updates contact time while handling this message (comes usually in
                // every 30 seconds), so we do it as well
                update_last_contact_times(tx, &self.target_ids, node_type)?;

                let affected = db::target::update_consistency_states(
                    tx,
                    self.target_ids
                        .into_iter()
                        .zip(self.new_states.iter().copied()),
                    node_type,
                )?;

                Ok(affected > 0)
            })
            .await?;

        log::debug!(
            "Updated target consistency states for {:?} nodes",
            self.node_type
        );

        if changed {
            notify_nodes(
                ctx,
                &[NodeType::Meta, NodeType::Storage, NodeType::Client],
                &RefreshTargetStates { ack_id: "".into() },
            )
            .await;
        }

        Ok(ChangeTargetConsistencyStatesResp {
            result: OpsErr::SUCCESS,
        })
    }
}
