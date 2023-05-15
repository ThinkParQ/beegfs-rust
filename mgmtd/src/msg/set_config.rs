use super::*;

pub(super) async fn handle(
    msg: msg::SetConfig,
    rcc: impl RequestConnectionController,
    mut ci: impl ComponentInteractor,
) -> Result<()> {
    let entries = msg.entries.clone();

    match ci
        .execute_db(move |tx| db::config::set(tx, msg.entries))
        .await
    {
        Ok(_) => {
            log::info!("Set {} config entries: {:?}", entries.len(), entries,);

            // update the cache
            if let Err(err) = ci.set_raw_config(entries).await {
                log::error!(
                    "Updated the persistent configuration, but updating the local config cache \
                     failed: {err}"
                )
            }

            rcc.respond(&msg::SetConfigResp {
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

            rcc.respond(&msg::SetConfigResp {
                result: OpsErr::INTERNAL,
            })
            .await
        }
    }
}
