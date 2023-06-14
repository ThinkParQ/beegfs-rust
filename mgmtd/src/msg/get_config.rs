use super::*;

pub(super) async fn handle(
    _msg: msg::GetConfig,
    ci: impl ComponentInteractor,
    _rcc: &impl RequestConnectionController,
) -> msg::GetConfigResp {
    match ::config::Source::get(&ci).await {
        Ok(entries) => msg::GetConfigResp { entries },
        Err(err) => {
            // TODO when config is replaced, fix error handling
            log::error!("Fetching config from source failed: {err}");
            msg::GetConfigResp {
                entries: HashMap::new(),
            }
        }
    }
}
