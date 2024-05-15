//! Functions for target management

use super::*;
use itertools::Itertools;
use std::borrow::Cow;
use std::cmp::Ordering;
use std::time::Duration;

/// A target entry.
///
/// Since this is also used for meta targets, pool_id is optional.
#[derive(Clone, Debug)]
pub(crate) struct Target {
    pub target_id: TargetID,
    pub node_uid: EntityUID,
    pub node_id: NodeID,
    pub pool_id: Option<StoragePoolID>,
    pub consistency: TargetConsistencyState,
    pub last_contact: Duration,
}

/// Retrieve a list of targets filtered by node type.
pub(crate) fn get_with_type(
    tx: &mut Transaction,
    node_type: NodeTypeServer,
) -> Result<Vec<Target>> {
    Ok(tx.query_map_collect(
        sql!(
            "SELECT target_id, node_uid, node_id, pool_id,
            consistency, (STRFTIME('%s', 'now') - STRFTIME('%s', n.last_contact))
            FROM all_targets_v AS t
            INNER JOIN nodes AS n USING(node_uid)
            WHERE t.node_type = ?1 AND t.node_id IS NOT NULL;"
        ),
        [node_type],
        |row| {
            Ok(Target {
                target_id: row.get(0)?,
                node_uid: row.get(1)?,
                node_id: row.get(2)?,
                pool_id: row.get(3)?,
                consistency: row.get(4)?,
                last_contact: Duration::from_secs(row.get(5)?),
            })
        },
    )?)
}

/// Retrieve the global UID for the given target ID and type.
///
/// # Return value
/// Returns `None` if the entry doesn't exist.
pub(crate) fn get_uid(
    tx: &mut Transaction,
    target_id: TargetID,
    node_type: NodeTypeServer,
) -> Result<Option<EntityUID>> {
    Ok(tx
        .query_row_cached(
            sql!("SELECT target_uid FROM all_targets_v WHERE target_id = ?1 AND node_type = ?2"),
            params![target_id, node_type],
            |row| row.get(0),
        )
        .optional()?)
}

/// Ensures that the list of given targets actually exists and returns an appropriate error if not.
pub(crate) fn validate_ids(
    tx: &mut Transaction,
    target_ids: &[TargetID],
    node_type: NodeTypeServer,
) -> Result<()> {
    let stmt = match node_type {
        NodeTypeServer::Meta => {
            sql!("SELECT COUNT(*) FROM meta_targets WHERE target_id IN rarray(?1)")
        }
        NodeTypeServer::Storage => {
            sql!("SELECT COUNT(*) FROM storage_targets WHERE target_id IN rarray(?1)")
        }
    };

    let count: usize =
        tx.query_row_cached(stmt, [&rarray_param(target_ids.iter().copied())], |row| {
            row.get(0)
        })?;

    match count.cmp(&target_ids.len()) {
        Ordering::Less => Err(anyhow!(TypedError::value_not_found(
            "target ID",
            target_ids.iter().join(", "),
        ))),
        Ordering::Greater => Err(anyhow!(
            "Target IDs have multiple matches: {} provided, {} selected",
            target_ids.len(),
            count
        )),
        Ordering::Equal => Ok(()),
    }
}

/// Inserts a new meta target.
///
/// BeeGFS doesn't really support meta targets at the moment, so there always must be exactly one
/// meta target per meta node with their IDs being the same.
pub(crate) fn insert_meta(
    tx: &mut Transaction,
    target_id: TargetID,
    alias: Option<&str>,
) -> Result<()> {
    let target_id = if target_id == 0 {
        misc::find_new_id(tx, "meta_targets", "target_id", 1..=0xFFFF)?
    } else if get_uid(tx, target_id, NodeTypeServer::Meta)?.is_some() {
        bail!(TypedError::value_exists("target ID", target_id));
    } else {
        target_id
    };

    insert(
        tx,
        target_id,
        alias,
        NodeTypeServer::Meta,
        Some(target_id.into()),
    )?;

    // If this is the first meta target, set it as meta root
    tx.execute(
        sql!("INSERT OR IGNORE INTO root_inode (target_id) VALUES (?1)"),
        params![target_id],
    )?;

    Ok(())
}

/// Inserts a new storage target if it doesn't exist yet.
///
/// Providing 0 for `target_id` chooses the ID automatically.
///
/// # Return value
/// Returns the ID of the new target.
pub(crate) fn insert_storage(
    tx: &mut Transaction,
    target_id: TargetID,
    alias: Option<&str>,
) -> Result<TargetID> {
    let target_id = if target_id == 0 {
        misc::find_new_id(tx, "storage_targets", "target_id", 1..=0xFFFF)?
    } else if get_uid(tx, target_id, NodeTypeServer::Storage)?.is_some() {
        return Ok(target_id);
    } else {
        target_id
    };

    insert(tx, target_id, alias, NodeTypeServer::Storage, None)?;

    Ok(target_id)
}

fn insert(
    tx: &mut Transaction,
    target_id: TargetID,
    alias: Option<&str>,
    node_type: NodeTypeServer,
    // This is optional because storage targets come "unmapped"
    node_id: Option<NodeID>,
) -> Result<()> {
    let alias = if let Some(alias) = alias {
        Cow::Borrowed(alias)
    } else {
        Cow::Owned(format!("target_{}_{}", node_type.as_sql_str(), target_id))
    };

    let new_uid = entity::insert(tx, EntityType::Target, alias.as_ref())?;

    tx.execute(
        sql!("INSERT INTO targets (target_uid, node_type) VALUES (?1, ?2)"),
        params![new_uid, node_type],
    )?;

    tx.execute(
        match node_type {
            NodeTypeServer::Meta => {
                sql!(
                    "INSERT INTO meta_targets (target_id, target_uid, node_id) VALUES (?1, ?2, ?3)"
                )
            }
            NodeTypeServer::Storage => {
                sql!(
                    "INSERT INTO storage_targets (target_id, target_uid, node_id)
                    VALUES (?1, ?2, ?3)"
                )
            }
        },
        params![target_id, new_uid, node_id],
    )?;

    Ok(())
}

/// Changes the consistency state for the given targets to new individual values.
///
/// # Return value
/// Returns the number of affected entries.
pub(crate) fn update_consistency_states(
    tx: &mut Transaction,
    changes: impl IntoIterator<Item = (TargetID, TargetConsistencyState)>,
    node_type: NodeTypeServer,
) -> Result<usize> {
    let mut update = tx.prepare_cached(sql!(
        "UPDATE targets SET consistency = ?3
        WHERE consistency != ?3 AND target_uid = (
            SELECT target_uid FROM all_targets_v WHERE target_id = ?1 AND node_type = ?2
        )"
    ))?;

    let mut updated = 0;
    for e in changes {
        updated += update.execute(params![e.0, node_type, e.1])?;
    }

    Ok(updated)
}

/// Change the storage pool of the given targets IDs to a new one.
pub(crate) fn update_storage_pools(
    tx: &mut Transaction,
    new_pool_id: StoragePoolID,
    target_ids: &[TargetID],
) -> Result<()> {
    if storage_pool::get_uid(tx, new_pool_id)?.is_none() {
        bail!(TypedError::value_not_found("pool ID", new_pool_id));
    }

    validate_ids(tx, target_ids, NodeTypeServer::Storage)?;

    tx.execute(
        sql!("UPDATE storage_targets SET pool_id = ?1 WHERE target_id IN rarray(?2)"),
        params![new_pool_id, &rarray_param(target_ids.iter().copied())],
    )?;

    Ok(())
}

/// Reset all storage targets belonging to the given storage pool to the default pool.
pub(crate) fn reset_storage_pool(
    tx: &mut Transaction,
    pool_id: StoragePoolID,
) -> rusqlite::Result<usize> {
    tx.execute_cached(
        sql!("UPDATE storage_targets SET pool_id = 1 WHERE pool_id = ?"),
        [pool_id],
    )
}

/// Assigns the given storage targets to a new node.
///
/// # Return value
/// Returns the number of affected entries
pub(crate) fn update_storage_node_mappings(
    tx: &mut Transaction,
    target_ids: &[TargetID],
    new_node_id: NodeID,
) -> Result<usize> {
    let mut stmt = tx.prepare_cached(sql!(
        "UPDATE storage_targets SET node_id = ?1 WHERE target_id = ?2"
    ))?;

    let mut updated = 0;
    for target_id in target_ids {
        updated += stmt.execute(params![new_node_id, target_id])?;
    }

    if updated != target_ids.len() {
        bail!(
            "Tried to map {} targets but only {updated} entries were updated",
            target_ids.len()
        );
    }

    Ok(updated)
}

/// Represents the storage capacities of a storage target.
///
/// Values are `None` if there is no information available.
pub(crate) struct TargetCapacities {
    pub total_space: Option<u64>,
    pub total_inodes: Option<u64>,
    pub free_space: Option<u64>,
    pub free_inodes: Option<u64>,
}

/// Retrieves the storage capacities for the given target IDs and updates them with new values.
///
/// # Return value
/// Vector of tuples containing the target ID and the pre-update capacity info.
pub(crate) fn get_and_update_capacities(
    tx: &mut Transaction,
    items: impl IntoIterator<Item = (TargetID, TargetCapacities)>,
    node_type: NodeTypeServer,
) -> Result<Vec<(TargetID, TargetCapacities)>> {
    let mut select = tx.prepare_cached(sql!(
        "SELECT total_space, total_inodes, free_space, free_inodes
        FROM all_targets_v
        WHERE target_id = ?1 AND node_type = ?2;"
    ))?;

    let mut update = tx.prepare_cached(sql!(
        "UPDATE targets
        SET total_space = ?1, total_inodes = ?2, free_space = ?3, free_inodes = ?4
        WHERE target_uid = (
            SELECT target_uid FROM all_targets_v WHERE target_id = ?5 AND node_type = ?6
        )"
    ))?;

    let mut old_values = vec![];

    for i in items {
        old_values.push(select.query_row(params![i.0, node_type], |row| {
            Ok((
                i.0,
                TargetCapacities {
                    total_space: row.get(0)?,
                    total_inodes: row.get(1)?,
                    free_space: row.get(2)?,
                    free_inodes: row.get(3)?,
                },
            ))
        })?);

        update.execute(params![
            i.1.total_space,
            i.1.total_inodes,
            i.1.free_space,
            i.1.free_inodes,
            i.0,
            node_type
        ])?;
    }

    Ok(old_values)
}

/// Deletes a storage target.
pub(crate) fn delete_storage(tx: &mut Transaction, target_id: TargetID) -> Result<()> {
    let affected = tx.execute_cached(
        sql!("DELETE FROM storage_targets WHERE target_id = ?1"),
        params![target_id],
    )?;

    check_affected_rows(affected, [1])
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn set_get_meta() {
        with_test_data(|tx| {
            super::insert_meta(tx, 1, Some("existing_meta_target")).unwrap_err();
            super::insert_meta(tx, 99, Some("new_meta_target")).unwrap();
            // existing alias
            super::insert_meta(tx, 99, Some("new_meta_target")).unwrap_err();

            let targets = super::get_with_type(tx, NodeTypeServer::Meta).unwrap();

            assert_eq!(5, targets.len());
        })
    }

    #[test]
    fn set_get_storage_and_map() {
        with_test_data(|tx| {
            let new_target_id = super::insert_storage(tx, 0, Some("new_storage_target")).unwrap();
            super::insert_storage(tx, 1000, Some("new_storage_target_2")).unwrap();

            // existing alias
            super::insert_storage(tx, 0, Some("new_storage_target")).unwrap_err();

            super::update_storage_node_mappings(tx, &[new_target_id, 1000], 1).unwrap();

            super::update_storage_node_mappings(tx, &[9999, 1], 1).unwrap_err();

            let targets = super::get_with_type(tx, NodeTypeServer::Storage).unwrap();

            assert_eq!(18, targets.len());

            assert!(targets.iter().any(|e| e.target_id == new_target_id));
            assert!(targets.iter().any(|e| e.target_id == 1000));
        })
    }
}
