//! Functions for storage pool management

use super::entity::EntityUID;
use super::*;
use rusqlite::OptionalExtension;

/// A storage pool entry.
#[derive(Clone, Debug)]
pub struct StoragePool {
    pub pool_id: StoragePoolID,
    pub alias: EntityAlias,
}

/// Retrieves all storage pool entries, including the default pool.
pub fn get_all(tx: &mut Transaction) -> DbResult<Vec<StoragePool>> {
    let mut stmt = tx.prepare_cached(
        r#"
        SELECT p.pool_id, alias FROM storage_pools AS p
        INNER JOIN entities AS e ON e.uid = p.pool_uid
        "#,
    )?;

    let res = stmt
        .query_map([], |row| {
            Ok(StoragePool {
                pool_id: row.get(0)?,
                alias: row.get(1)?,
            })
        })?
        .try_collect()?;

    Ok(res)
}

/// Retrieve the global UID for the given storage pool ID.
///
/// # Return value
/// Returns `None` if the entry doesn't exist.
pub fn get_uid(
    tx: &mut Transaction,
    pool_id: StoragePoolID,
) -> rusqlite::Result<Option<EntityUID>> {
    tx.query_row_cached(
        "SELECT pool_uid FROM storage_pools WHERE pool_id = ?1",
        [pool_id],
        |row| row.get(0),
    )
    .optional()
}

/// Inserts a storage pool entry and assigns the given targets and buddy groups to the new pool.
///
/// # Return value
/// Returns the newly created storage pool ID.
pub fn insert(
    tx: &mut Transaction,
    pool_id: StoragePoolID,
    pool_uid: EntityUID,
) -> rusqlite::Result<usize> {
    tx.execute(
        "INSERT INTO storage_pools (pool_id, pool_uid) VALUES (?1, ?2)",
        params![pool_id, pool_uid],
    )
}

/// Deletes a storage pool entry
pub fn delete(tx: &mut Transaction, pool_id: StoragePoolID) -> rusqlite::Result<usize> {
    tx.execute_checked(
        "DELETE FROM storage_pools WHERE pool_id = ?1",
        params![pool_id],
        1..=1,
    )
}

#[cfg(test)]
mod test {
    use super::*;
    use entity::EntityType;

    #[test]
    fn insert_get_delete() {
        with_test_data(|tx| {
            let pool_uid = entity::insert(tx, EntityType::StoragePool, &"new_pool".into()).unwrap();
            assert_eq!(4, get_all(tx).unwrap().len());
            insert(tx, 123.into(), pool_uid).unwrap();
            assert_eq!(5, get_all(tx).unwrap().len());
            insert(tx, 124.into(), pool_uid).unwrap_err();
            assert_eq!(5, get_all(tx).unwrap().len());
            delete(tx, 123.into()).unwrap();
            delete(tx, 123.into()).unwrap_err();
            assert_eq!(4, get_all(tx).unwrap().len());
        })
    }

    #[test]
    fn get_uid() {
        with_test_data(|tx| {
            assert_eq!(
                Some(EntityUID::from(401002)),
                storage_pool::get_uid(tx, 2.into()).unwrap()
            );
            assert_eq!(None, storage_pool::get_uid(tx, 1234.into()).unwrap());
        })
    }
}
