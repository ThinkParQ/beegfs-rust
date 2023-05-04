use super::*;
use crate::db::NonexistingKey;

pub(super) async fn handle(
    msg: msg::UnmapStorageTarget,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    match hnd
        .execute_db(move |tx| db::targets::delete_storage(tx, msg.target_id))
        .await
    {
        Ok(_) => {
            log::info!("Removed storage target {}", msg.target_id,);

            hnd.notify_nodes(&msg::RefreshCapacityPools { ack_id: "".into() })
                .await;

            chn.respond(&msg::UnmapStorageTargetResp {
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

            chn.respond(&msg::UnmapStorageTargetResp {
                result: match err.downcast_ref() {
                    Some(NonexistingKey(_)) => OpsErr::UNKNOWN_TARGET,
                    None => OpsErr::INTERNAL,
                },
            })
            .await
        }
    }
}
