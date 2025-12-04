use super::*;
use shared::bee_msg::target::*;

impl HandleWithResponse for RegisterTarget {
    type Response = RegisterTargetResp;

    async fn handle(self, app: &impl App, _req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(app)?;

        let registration_disable = app.static_info().user_config.registration_disable;

        let (id, is_new) = app
            .write_tx(move |tx| {
                // Do not do anything if the target already exists
                if let Some(id) = try_resolve_num_id(
                    tx,
                    EntityType::Target,
                    NodeType::Storage,
                    self.target_id.into(),
                )? {
                    return Ok((id.num_id().try_into()?, false));
                }

                if registration_disable {
                    bail!("Registration of new targets is not allowed");
                }

                Ok((
                    db::target::insert_storage(
                        tx,
                        self.target_id,
                        Some(format!("target_{}", std::str::from_utf8(&self.alias)?).try_into()?),
                    )?,
                    true,
                ))
            })
            .await?;

        if is_new {
            log::info!("Registered new storage target with Id {id}");
        } else {
            log::debug!("Re-registered existing storage target with Id {id}");
        }

        Ok(RegisterTargetResp { id })
    }
}
