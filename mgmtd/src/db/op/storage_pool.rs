//! Functions for storage pool managementE

use super::*;
use rusqlite::OptionalExtension;

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
    fn get_uid() {
        with_test_data(|tx| {
            assert_eq!(
                Some(EntityUID::from(401002u64)),
                storage_pool::get_uid(tx, 2).unwrap()
            );
            assert_eq!(None, storage_pool::get_uid(tx, 1234).unwrap());
        })
    }
}
