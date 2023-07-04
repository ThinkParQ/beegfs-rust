use super::*;

pub(super) async fn handle(
    msg: msg::UnmapTarget,
    ci: impl ComponentInteractor,
    _rcc: &impl RequestConnectionController,
) -> msg::UnmapTargetResp {
    match ci
        .db_op(move |tx| {
            // Check given target ID exists
            db::target::get_uid(tx, msg.target_id, NodeTypeServer::Storage)?
                .ok_or_else(|| DbError::value_not_found("target ID", msg.target_id))?;

            db::target::delete_storage(tx, msg.target_id)
        })
        .await
    {
        Ok(_) => {
            log::info!("Removed storage target {}", msg.target_id,);

            ci.notify_nodes(&msg::RefreshCapacityPools { ack_id: "".into() })
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
