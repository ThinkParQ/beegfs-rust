use super::*;
use shared::config::RegistrationEnable;

pub(super) async fn handle(
    msg: msg::RegisterStorageTarget,
    rcc: impl RequestConnectionController,
    ci: impl ComponentInteractor,
) -> Result<()> {
    match async move {
        if !ci.get_config::<RegistrationEnable>() {
            bail!("Registration of new targets is disabled");
        }

        ci.execute_db(move |tx| {
            db::targets::insert_or_ignore_storage(
                tx,
                match msg.id {
                    TargetID::ZERO => None,
                    n => Some(n),
                },
                &msg.alias,
            )
        })
        .await
    }
    .await
    {
        Ok(id) => {
            log::info!("Registered storage target {id}");
            rcc.respond(&msg::RegisterStorageTargetResp { id }).await
        }
        Err(err) => {
            log::error!("Registering storage target {} failed:\n{:?}", msg.id, err);
            rcc.respond(&msg::RegisterStorageTargetResp { id: TargetID::ZERO })
                .await
        }
    }
}
