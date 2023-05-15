use super::*;

pub(super) async fn handle(
    msg: msg::RemoveStoragePool,
    rcc: impl RequestConnectionController,
    ci: impl ComponentInteractor,
) -> Result<()> {
    match async {
        ci.execute_db(move |tx| db::storage_pools::delete(tx, msg.id))
            .await?;

        Ok(()) as Result<_>
    }
    .await
    {
        Ok(_) => {
            log::info!("Storage pool {} removed", msg.id,);

            ci.notify_nodes(&msg::RefreshStoragePools { ack_id: "".into() })
                .await;

            rcc.respond(&msg::RemoveStoragePoolResp {
                result: OpsErr::SUCCESS,
            })
            .await
        }
        Err(err) => {
            log::error!("Removing storage pool {} failed:\n{:?}", msg.id, err);

            rcc.respond(&msg::RemoveStoragePoolResp {
                result: OpsErr::INTERNAL,
            })
            .await
        }
    }
}
