//! Functions for storage pool managementE

use super::*;

/// Inserts a storage pool entry and assigns the given targets and buddy groups to the new pool.
pub(crate) fn insert(tx: &Transaction, pool_id: PoolId, alias: &Alias) -> Result<(Uid, PoolId)> {
    let pool_id = if pool_id == 0 {
        misc::find_new_id(tx, "pools", "pool_id", NodeType::Storage, 1..=0xFFFF)?
    } else if try_resolve_num_id(tx, EntityType::Pool, NodeType::Storage, pool_id.into())?.is_some()
    {
        bail!(TypedError::value_exists("numeric pool id", pool_id));
    } else {
        pool_id
    };

    let new_uid = entity::insert(tx, EntityType::Pool, alias)?;

    let affected = tx.execute(
        sql!("INSERT INTO pools (pool_id, pool_uid, node_type) VALUES (?1, ?2, ?3)"),
        params![pool_id, new_uid, NodeType::Storage.sql_variant()],
    )?;

    check_affected_rows(affected, [1])?;

    Ok((new_uid, pool_id))
}
