use super::*;

/// Sets the entity alias for any entity
pub(crate) async fn set_alias(
    ctx: &Context,
    req: pm::SetAliasRequest,
) -> Result<pm::SetAliasResponse> {
    // Parse proto msg
    let entity_type: EntityType = req.entity_type().try_into()?;
    let entity_id: EntityId = required_field(req.entity_id)?.try_into()?;
    let alias: Alias = req.new_alias.try_into()?;

    ctx.db
        .op(move |tx| {
            let entity = entity_id.resolve(tx, entity_type)?;

            if *entity.node_type() == NodeType::Client {
                bail!("Client updates are not supported")
            }

            // Check that the alias is not in use yet
            let et: Option<EntityType> = tx
                .query_row_cached(
                    sql!("SELECT entity_type FROM entities WHERE alias = ?1"),
                    [alias.as_ref()],
                    |row| EntityType::from_row(row, 0),
                )
                .optional()?;

            if let Some(et) = et {
                bail!("Alias {} is already in use by a {}", alias, et.sql_str());
            }

            tx.execute_cached(
                sql!("UPDATE entities SET alias = ?1 WHERE uid = ?2"),
                params![alias.as_ref(), entity.uid],
            )?;

            Ok(())
        })
        .await?;

    Ok(pm::SetAliasResponse {})
}
