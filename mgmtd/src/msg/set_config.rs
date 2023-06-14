use super::*;

pub(super) async fn handle(
    msg: msg::SetConfig,
    mut ci: impl ComponentInteractor,
    _rcc: &impl RequestConnectionController,
) -> msg::SetConfigResp {
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

            msg::SetConfigResp {
                result: OpsErr::SUCCESS,
            }
        }

        Err(err) => {
            log_error_chain!(err, "Setting {} config entries failed", entries.len());

            msg::SetConfigResp {
                result: OpsErr::INTERNAL,
            }
        }
    }
}
