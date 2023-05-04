use super::*;

pub(super) async fn handle(
    _msg: msg::GetConfig,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    chn.respond(&match ::config::Source::get(&hnd).await {
        Ok(entries) => msg::GetAllConfigResp { entries },
        Err(err) => {
            log::error!("Fetching config from source failed: {err}");
            msg::GetAllConfigResp {
                entries: HashMap::new(),
            }
        }
    })
    .await
}
