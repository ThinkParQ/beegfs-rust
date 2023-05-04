use super::*;

pub(super) async fn handle(
    msg: msg::RemoveStoragePool,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    match async {
        hnd.execute_db(move |tx| db::storage_pools::delete(tx, msg.id))
            .await?;

        Ok(()) as Result<_>
    }
    .await
    {
        Ok(_) => {
            log::info!("Storage pool {} removed", msg.id,);

            hnd.notify_nodes(&msg::RefreshStoragePools { ack_id: "".into() })
                .await;

            chn.respond(&msg::RemoveStoragePoolResp {
                result: OpsErr::SUCCESS,
            })
            .await
        }
        Err(err) => {
            log::error!("Removing storage pool {} failed:\n{:?}", msg.id, err);

            chn.respond(&msg::RemoveStoragePoolResp {
                result: OpsErr::INTERNAL,
            })
            .await
        }
    }
}
