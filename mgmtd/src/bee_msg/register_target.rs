use super::*;
use shared::bee_msg::target::*;

impl HandleWithResponse for RegisterTarget {
    type Response = RegisterTargetResp;

    async fn handle(self, app: &impl App, _req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(app)?;

        let registration_disable = app.static_info().user_config.registration_disable;

        let (id, is_new) = app
            .write_tx(move |tx| {
                let reg_token = str::from_utf8(&self.reg_token)?;

                if let Some(id) = try_resolve_num_id(
                    tx,
                    EntityType::Target,
                    NodeType::Storage,
                    self.target_id.into(),
                )? {
                    // If the target already exists, check if the registration tokens match
                    let stored_reg_token: Option<String> = tx.query_row(
                        sql!(
                            "SELECT reg_token FROM targets
                            WHERE target_id = ?1 AND node_type = ?2"
                        ),
                        rusqlite::params![id.num_id(), NodeType::Storage.sql_variant()],
                        |row| row.get(0),
                    )?;

                    if let Some(ref t) = stored_reg_token
                        && t != reg_token
                    {
                        bail!(
                            "Storage target {id} has already been registered and its \
registration token ({reg_token}) does not match the stored token ({t})"
                        );
                    } else if stored_reg_token.is_none() {
                        tx.execute(
                            sql!(
                                "UPDATE targets SET reg_token = ?1
                                WHERE target_id = ?2 AND node_type = ?3"
                            ),
                            rusqlite::params![
                                reg_token,
                                id.num_id(),
                                NodeType::Storage.sql_variant()
                            ],
                        )?;
                    }

                    return Ok((id.num_id().try_into()?, false));
                }

                if registration_disable {
                    bail!("Registration of new targets is not allowed");
                }

                Ok((
                    db::target::insert_storage(tx, self.target_id, Some(reg_token))?,
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
