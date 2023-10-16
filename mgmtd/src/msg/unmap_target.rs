use super::*;
use shared::msg::refresh_capacity_pools::RefreshCapacityPools;
use shared::msg::unmap_target::{UnmapTarget, UnmapTargetResp};
use shared::types::{NodeType, NodeTypeServer};

pub(super) async fn handle(
    msg: UnmapTarget,
    ctx: &Context,
    _req: &impl Request,
) -> UnmapTargetResp {
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
                &RefreshCapacityPools { ack_id: "".into() },
            )
            .await;

            UnmapTargetResp {
                result: OpsErr::SUCCESS,
            }
        }
        Err(err) => {
            log_error_chain!(err, "Unmapping storage target {} failed", msg.target_id);

            UnmapTargetResp {
                result: OpsErr::INTERNAL,
            }
        }
    }
}
