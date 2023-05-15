use super::*;

pub(super) async fn handle(
    _msg: msg::GetConfig,
    rcc: impl RequestConnectionController,
    ci: impl ComponentInteractor,
) -> Result<()> {
    rcc.respond(&match ::config::Source::get(&ci).await {
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
