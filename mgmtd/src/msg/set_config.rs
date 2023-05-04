use super::*;

pub(super) async fn handle(
    msg: msg::SetConfig,
    chn: impl RequestChannel,
    mut hnd: impl ComponentHandles,
) -> Result<()> {
    let entries = msg.entries.clone();

    match hnd
        .execute_db(move |tx| db::config::set(tx, msg.entries))
        .await
    {
        Ok(_) => {
            log::info!("Set {} config entries: {:?}", entries.len(), entries,);

            // update the cache
            if let Err(err) = hnd.set_raw_config(entries).await {
                log::error!(
                    "Updated the persistent configuration, but updating the local config cache \
                     failed: {err}"
                )
            }

            chn.respond(&msg::SetConfigResp {
                result: OpsErr::SUCCESS,
            })
            .await
        }

        Err(err) => {
            log::error!(
                "Setting {} config entries failed:\n{:?}",
                entries.len(),
                err
            );

            chn.respond(&msg::SetConfigResp {
                result: OpsErr::INTERNAL,
            })
            .await
        }
    }
}
