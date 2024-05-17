use super::*;
use crate::db;
use crate::db::entity::check_alias;
use crate::types::{EntityType, SqliteStr};
use anyhow::bail;
use rusqlite::OptionalExtension;

pub(crate) async fn set_alias(ctx: &Context, req: SetAliasRequest) -> Result<SetAliasResponse> {
    ctx.db
        .op(move |tx| {
            check_alias(&req.new_alias)?;

            let uid = db::misc::uid_from_proto_entity_id(tx, req.entity_id.unwrap())?;

            let et: Option<EntityType> = tx
                .query_row_cached(
                    sql!("SELECT entity_type FROM entities WHERE alias = ?1"),
                    [&req.new_alias],
                    |row| EntityType::from_row(row, 0),
                )
                .optional()?;

            if let Some(et) = et {
                bail!(
                    "Alias {} is already in use by a {}",
                    req.new_alias,
                    et.sql_str()
                );
            }

            let affected = tx.execute_cached(
                sql!("UPDATE entities SET alias = ?1 WHERE uid = ?2"),
                params![req.new_alias, uid],
            )?;

            if affected != 1 {
                bail!("Entity with UID {} not found", uid);
            }

            Ok(())
        })
        .await?;

    Ok(SetAliasResponse {})
}
