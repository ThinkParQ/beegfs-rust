use super::*;

pub(super) async fn handle(
    msg: msg::RegisterStorageTarget,
    ci: impl ComponentInteractor,
    _rcc: &impl RequestConnectionController,
) -> msg::RegisterStorageTargetResp {
    match async move {
        if !ci.get_config().registration_enable {
            bail!("Registration of new targets is not allowed");
        }

        Ok(ci
            .db_op(move |tx| {
                // TODO add checks for the alias?
                // if db::targets::get_uid_optional(tx, msg.target_id,
                // NodeTypeServer::Storage)?.is_some()
                //     && db::entities::get_uid_by_alias_optional(tx, &msg.alias)?.is_some()
                // {

                // }

                db::target::insert_or_ignore_storage(
                    tx,
                    match msg.target_id {
                        TargetID::ZERO => None,
                        n => Some(n),
                    },
                    &msg.alias,
                )
            })
            .await?)
    }
    .await
    {
        Ok(id) => {
            log::info!("Registered storage target {id}");
            msg::RegisterStorageTargetResp { id }
        }
        Err(err) => {
            log_error_chain!(err, "Registering storage target {} failed", msg.target_id);
            msg::RegisterStorageTargetResp { id: TargetID::ZERO }
        }
    }
}
