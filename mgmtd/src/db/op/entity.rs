//! Functions for entity management
//!
//! An entity is a globally unique entry, spanning nodes, targets, buddy groups and storage pools.
//! It ensures that the assigned UID and alias are globally unique.
//!
//! The db schema ensures that each entry in the respective tables of the entities mentioned above
//! requires an entity entry first.

use super::*;
use rusqlite::OptionalExtension;

/// Get the UID of an entity by the given alias.
///
/// # Return value
/// Returns the entities UID or `None` if the given alias doesn't exist.
pub(crate) fn get_uid(tx: &mut Transaction, alias: &str) -> Result<Option<EntityUID>> {
    let uid = tx
        .query_row_cached(
            sql!("SELECT uid FROM entities WHERE alias = ?1"),
            [alias],
            |row| row.get(0),
        )
        .optional()?;

    Ok(uid)
}

/// Inserts a new entity.
///
/// # Return value
/// Returns the newly inserted entity UID
pub(crate) fn insert(
    tx: &mut Transaction,
    entity_type: EntityType,
    alias: &str,
) -> Result<EntityUID> {
    let affected = tx.execute_cached(
        sql!("INSERT INTO entities (entity_type, alias) VALUES (?1, ?2)"),
        params![entity_type, alias],
    )?;

    check_affected_rows(affected, [1])?;

    Ok(tx.last_insert_rowid())
}

/// Updates the alias of an entity.
pub(crate) fn update_alias(tx: &mut Transaction, uid: EntityUID, new_alias: &str) -> Result<()> {
    let affected = tx.execute_cached(
        sql!("UPDATE entities SET alias = ?1 WHERE uid = ?2"),
        params![new_alias, uid],
    )?;

    check_affected_rows(affected, [1])
}
