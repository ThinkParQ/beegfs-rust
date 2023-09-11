use super::*;

pub(super) async fn handle(
    msg: msg::RegisterTarget,
    ctx: &impl AppContext,
    _req: &impl Request,
) -> msg::RegisterTargetResp {
    match async move {
        if !ctx.runtime_info().config.registration_enable {
            bail!("Registration of new targets is not allowed");
        }

        Ok(ctx
            .db_op(move |tx| {
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
            msg::RegisterTargetResp { id }
        }
        Err(err) => {
            log_error_chain!(err, "Registering storage target {} failed", msg.target_id);
            msg::RegisterTargetResp { id: TargetID::ZERO }
        }
    }
}
