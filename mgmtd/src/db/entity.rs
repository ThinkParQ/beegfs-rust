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
pub(crate) fn get_uid(tx: &Transaction, alias: &str) -> Result<Option<Uid>> {
    let uid = tx
        .query_row_cached(
            sql!("SELECT uid FROM entities WHERE alias = ?1"),
            [alias],
            |row| row.get(0),
        )
        .optional()?;

    Ok(uid)
}

/// Get the alias of an entity by the given UID.
///
/// # Return value
/// Returns the entities alias or `None` if the given alias doesn't exist.
pub(crate) fn get_alias(tx: &Transaction, uid: Uid) -> Result<Option<String>> {
    let uid = tx
        .query_row_cached(
            sql!("SELECT alias FROM entities WHERE uid = ?1"),
            [uid],
            |row| row.get(0),
        )
        .optional()?;

    Ok(uid)
}

/// Inserts a new entity.
///
/// # Return value
/// Returns the newly inserted entity UID
pub(crate) fn insert(tx: &Transaction, entity_type: EntityType, alias: &Alias) -> Result<Uid> {
    // Check alias is free
    if get_uid(tx, alias.as_ref())?.is_some() {
        bail!(TypedError::value_exists("Alias", alias));
    }

    let affected = tx.execute_cached(
        sql!("INSERT INTO entities (entity_type, alias) VALUES (?1, ?2)"),
        params![entity_type.sql_variant(), alias.as_ref()],
    )?;

    check_affected_rows(affected, [1])?;

    Ok(tx.last_insert_rowid())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn get() {
        with_test_data(|tx| {
            let uid = get_uid(tx, "management").unwrap().unwrap();
            let alias = get_alias(tx, uid).unwrap().unwrap();

            assert_eq!("management", alias);
        })
    }
}
