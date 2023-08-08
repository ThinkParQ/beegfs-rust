//! Functions for entity management
//!
//! An entity is a globally unique entry, spanning nodes, targets, buddy groups and storage pools.
//! It ensures that the assigned UID and alias are globally unique.
//!
//! The db schema ensures that each entry in the respective tables of the entities mentioned above
//! requires an entity entry first.

use super::*;
use rusqlite::OptionalExtension;
use shared::{impl_enum_to_sql_str, EntityAlias};

/// The entity type.
#[derive(Clone, Debug)]
pub enum EntityType {
    Node,
    Target,
    BuddyGroup,
    StoragePool,
}

impl_enum_to_sql_str!(EntityType,
    Node => "node",
    Target => "target",
    BuddyGroup => "buddy_group",
    StoragePool => "storage_pool",
);

/// Get the UID of an entity by the given alias.
///
/// # Return value
/// Returns the entities UID or `None` if the given alias doesn't exist.
pub(crate) fn get_uid(tx: &mut Transaction, alias: &EntityAlias) -> DbResult<Option<EntityUID>> {
    let uid = tx
        .query_row_cached(
            "SELECT uid FROM entities WHERE alias = ?1",
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
    alias: &EntityAlias,
) -> DbResult<EntityUID> {
    tx.execute_checked_cached(
        "INSERT INTO entities (entity_type, alias) VALUES (?1, ?2)",
        params![entity_type, alias],
        1..=1,
    )?;

    Ok(tx.last_insert_rowid().into())
}

/// Updates the alias of an entity.
pub(crate) fn update_alias(
    tx: &mut Transaction,
    uid: EntityUID,
    new_alias: &EntityAlias,
) -> DbResult<()> {
    tx.execute_checked_cached(
        "UPDATE entities SET alias = ?1 WHERE uid = ?2",
        params![new_alias, uid],
        1..=1,
    )?;

    Ok(())
}
