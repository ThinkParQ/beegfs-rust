//! Functions for entity management
//!
//! An entity is a globally unique entry, spanning nodes, targets, buddy groups and storage pools.
//! It ensures that the assigned UID and alias are globally unique.
//!
//! The db schema ensures that each entry in the respective tables of the entities mentioned above
//! requires an entity entry first.

use super::*;
use regex::Regex;
use rusqlite::OptionalExtension;
use std::sync::OnceLock;

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

/// Get the alias of an entity by the given UID.
///
/// # Return value
/// Returns the entities alias or `None` if the given alias doesn't exist.
pub(crate) fn get_alias(tx: &mut Transaction, uid: EntityUID) -> Result<Option<String>> {
    let uid = tx
        .query_row_cached(
            sql!("SELECT alias FROM entities WHERE uid = ?1"),
            [uid],
            |row| row.get(0),
        )
        .optional()?;

    Ok(uid)
}

static REGEX: OnceLock<Regex> = OnceLock::new();

/// Checks an alias for validity and returns a user friendly error if not
pub(crate) fn check_alias(alias: &str) -> Result<()> {
    let re = REGEX
        .get_or_init(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9-_.]+$").expect("Regex must be valid"));
    if !re.is_match(alias) {
        bail!("invalid alias '{alias}': must start with a letter and may only contain letters, digits, '-', '_' and '.'");
    }

    Ok(())
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
    check_alias(alias)?;

    // Check alias is free
    if get_uid(tx, alias)?.is_some() {
        bail!(TypedError::value_exists("Alias", alias));
    }

    let affected = tx.execute_cached(
        sql!("INSERT INTO entities (entity_type, alias) VALUES (?1, ?2)"),
        params![entity_type.sql_str(), alias],
    )?;

    check_affected_rows(affected, [1])?;

    Ok(tx.last_insert_rowid() as u64)
}

/// Updates the alias of an entity.
pub(crate) fn update_alias(tx: &mut Transaction, uid: EntityUID, new_alias: &str) -> Result<()> {
    check_alias(new_alias)?;

    let affected = tx.execute_cached(
        sql!("UPDATE entities SET alias = ?1 WHERE uid = ?2"),
        params![new_alias, uid],
    )?;

    check_affected_rows(affected, [1])
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

    #[test]
    fn check_alias() {
        super::check_alias("aaa").unwrap();
        super::check_alias("BBB").unwrap();
        super::check_alias("a-zA-Z0-9._-").unwrap();

        super::check_alias("1a").unwrap_err();
        super::check_alias("-a").unwrap_err();
        super::check_alias("&").unwrap_err();
        super::check_alias("*").unwrap_err();
        super::check_alias(" ").unwrap_err();
        super::check_alias("\t").unwrap_err();
        super::check_alias(":").unwrap_err();
    }

    #[test]
    fn alias_db_charset() {
        with_test_data(|tx| {
            // Case sensitivity
            insert(tx, EntityType::Node, "aaa").unwrap();
            insert(tx, EntityType::Node, "Aaa").unwrap();
            insert(tx, EntityType::Node, "BBB").unwrap();
            insert(tx, EntityType::Node, "bbb").unwrap();

            assert!(get_uid(tx, "AAA").unwrap().is_none());
            assert!(get_uid(tx, "bBb").unwrap().is_none());

            // Character set
            insert(tx, EntityType::Node, "a-zA-Z0-9._-").unwrap();

            insert(tx, EntityType::Node, "&").unwrap_err();
            insert(tx, EntityType::Node, "*").unwrap_err();
            insert(tx, EntityType::Node, " ").unwrap_err();
            insert(tx, EntityType::Node, "\t").unwrap_err();
            insert(tx, EntityType::Node, ":").unwrap_err();
        })
    }
}
