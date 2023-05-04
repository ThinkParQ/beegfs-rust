use super::*;
use shared::config::RegistrationEnable;

pub(super) async fn handle(
    msg: msg::RegisterStorageTarget,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    match async move {
        if !hnd.get_config::<RegistrationEnable>() {
            bail!("Registration of new targets is disabled");
        }

        hnd.execute_db(move |tx| db::targets::insert_storage_if_new(tx, msg.id, msg.alias))
            .await
    }
    .await
    {
        Ok(id) => {
            log::info!("Registered storage target {id}");
            chn.respond(&msg::RegisterStorageTargetResp { id }).await
        }
        Err(err) => {
            log::error!("Registering storage target {} failed:\n{:?}", msg.id, err);
            chn.respond(&msg::RegisterStorageTargetResp { id: TargetID::ZERO })
                .await
        }
    }
}
