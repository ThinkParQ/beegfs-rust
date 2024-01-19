//! Functions for storage pool managementE

use super::*;
use rusqlite::OptionalExtension;

/// A storage pool entry.
#[derive(Clone, Debug)]
pub(crate) struct StoragePool {
    pub pool_id: StoragePoolID,
    pub alias: String,
}

/// Retrieves all storage pool entries, including the default pool.
pub(crate) fn get_all(tx: &mut Transaction) -> Result<Vec<StoragePool>> {
    Ok(tx.query_map_collect(
        sql!(
            "SELECT p.pool_id, alias FROM storage_pools AS p
            INNER JOIN entities AS e ON e.uid = p.pool_uid"
        ),
        [],
        |row| {
            Ok(StoragePool {
                pool_id: row.get(0)?,
                alias: row.get(1)?,
            })
        },
    )?)
}

/// Retrieve the global UID for the given storage pool ID.
///
/// # Return value
/// Returns `None` if the entry doesn't exist.
pub(crate) fn get_uid(tx: &mut Transaction, pool_id: StoragePoolID) -> Result<Option<EntityUID>> {
    Ok(tx
        .query_row_cached(
            sql!("SELECT pool_uid FROM storage_pools WHERE pool_id = ?1"),
            [pool_id],
            |row| row.get(0),
        )
        .optional()?)
}

/// Inserts a storage pool entry and assigns the given targets and buddy groups to the new pool.
pub(crate) fn insert(
    tx: &mut Transaction,
    pool_id: StoragePoolID,
    pool_uid: EntityUID,
) -> Result<()> {
    let affected = tx.execute(
        sql!("INSERT INTO storage_pools (pool_id, pool_uid) VALUES (?1, ?2)"),
        params![pool_id, pool_uid],
    )?;

    check_affected_rows(affected, [1])
}

/// Deletes a storage pool entry
pub(crate) fn delete(tx: &mut Transaction, pool_id: StoragePoolID) -> Result<()> {
    let affected = tx.execute(
        sql!("DELETE FROM storage_pools WHERE pool_id = ?1"),
        params![pool_id],
    )?;

    check_affected_rows(affected, [1])
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn insert_get_delete() {
        with_test_data(|tx| {
            let pool_uid = entity::insert(tx, EntityType::StoragePool, "new_pool").unwrap();
            assert_eq!(4, get_all(tx).unwrap().len());
            insert(tx, 123, pool_uid).unwrap();
            assert_eq!(5, get_all(tx).unwrap().len());
            insert(tx, 124, pool_uid).unwrap_err();
            assert_eq!(5, get_all(tx).unwrap().len());
            delete(tx, 123).unwrap();
            delete(tx, 123).unwrap_err();
            assert_eq!(4, get_all(tx).unwrap().len());
        })
    }

    #[test]
    fn get_uid() {
        with_test_data(|tx| {
            assert_eq!(
                Some(EntityUID::from(401002)),
                storage_pool::get_uid(tx, 2).unwrap()
            );
            assert_eq!(None, storage_pool::get_uid(tx, 1234).unwrap());
        })
    }
}
