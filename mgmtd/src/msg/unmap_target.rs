use super::*;
use shared::types::{NodeType, NodeTypeServer};

pub(super) async fn handle(
    msg: msg::UnmapTarget,
    ctx: &Context,
    _req: &impl Request,
) -> msg::UnmapTargetResp {
    match ctx
        .db
        .op(move |tx| {
            // Check given target ID exists
            db::target::get_uid(tx, msg.target_id, NodeTypeServer::Storage)?
                .ok_or_else(|| TypedError::value_not_found("target ID", msg.target_id))?;

            db::target::delete_storage(tx, msg.target_id)
        })
        .await
    {
        Ok(_) => {
            log::info!("Removed storage target {}", msg.target_id,);

            notify_nodes(
                ctx,
                &[NodeType::Meta],
                &msg::RefreshCapacityPools { ack_id: "".into() },
            )
            .await;

            msg::UnmapTargetResp {
                result: OpsErr::SUCCESS,
            }
        }
        Err(err) => {
            log_error_chain!(err, "Unmapping storage target {} failed", msg.target_id);

            msg::UnmapTargetResp {
                result: OpsErr::INTERNAL,
            }
        }
    }
}
