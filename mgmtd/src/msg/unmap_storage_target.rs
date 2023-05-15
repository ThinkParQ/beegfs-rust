use super::*;
use crate::db::NonexistingKey;

pub(super) async fn handle(
    msg: msg::UnmapStorageTarget,
    rcc: impl RequestConnectionController,
    ci: impl ComponentInteractor,
) -> Result<()> {
    match ci
        .execute_db(move |tx| db::targets::delete_storage(tx, msg.target_id))
        .await
    {
        Ok(_) => {
            log::info!("Removed storage target {}", msg.target_id,);

            ci.notify_nodes(&msg::RefreshCapacityPools { ack_id: "".into() })
                .await;

            rcc.respond(&msg::UnmapStorageTargetResp {
                result: OpsErr::SUCCESS,
            })
            .await
        }
        Err(err) => {
            log::error!(
                "Unmapping storage target {} failed:\n{:?}",
                msg.target_id,
                err
            );

            rcc.respond(&msg::UnmapStorageTargetResp {
                result: match err.downcast_ref() {
                    Some(NonexistingKey(_)) => OpsErr::UNKNOWN_TARGET,
                    None => OpsErr::INTERNAL,
                },
            })
            .await
        }
    }
}
