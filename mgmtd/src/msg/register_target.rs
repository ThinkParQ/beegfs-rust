use super::*;

pub(super) async fn handle(
    msg: msg::RegisterTarget,
    ctx: &Context,
    _req: &impl Request,
) -> msg::RegisterTargetResp {
    match async move {
        if !ctx.info.config.registration_enable {
            bail!("Registration of new targets is not allowed");
        }

        ctx.db
            .op(move |tx| {
                db::target::insert_or_ignore_storage(
                    tx,
                    match msg.target_id {
                        0 => None,
                        n => Some(n),
                    },
                    std::str::from_utf8(&msg.alias)?,
                )
            })
            .await
    }
    .await
    {
        Ok(id) => {
            log::info!("Registered storage target {id}");
            msg::RegisterTargetResp { id }
        }
        Err(err) => {
            log_error_chain!(err, "Registering storage target {} failed", msg.target_id);
            msg::RegisterTargetResp { id: 0 }
        }
    }
}
