//! Functions for target management

use super::*;
use itertools::Itertools;
use std::cmp::Ordering;

/// Ensures that the list of given targets actually exists and returns an appropriate error if not.
pub(crate) fn validate_ids(
    tx: &Transaction,
    target_ids: &[TargetId],
    node_type: NodeTypeServer,
) -> Result<()> {
    let count: usize = tx.query_row_cached(
        sql!("SELECT COUNT(*) FROM targets WHERE target_id IN rarray(?1) AND node_type = ?2"),
        params![
            &rarray_param(target_ids.iter().copied()),
            node_type.sql_variant(),
        ],
        |row| row.get(0),
    )?;

    match count.cmp(&target_ids.len()) {
        Ordering::Less => Err(anyhow!(TypedError::value_not_found(
            "numeric target id",
            target_ids.iter().join(", "),
        ))),
        Ordering::Greater => Err(anyhow!(
            "numeric target ids have multiple matches: {} provided, {} selected",
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
    tx: &Transaction,
    target_id: TargetId,
    alias: Option<Alias>,
) -> Result<()> {
    let target_id = if target_id == 0 {
        misc::find_new_id(tx, "targets", "target_id", NodeType::Meta, 1..=0xFFFF)?
    } else if try_resolve_num_id(tx, EntityType::Target, NodeType::Meta, target_id.into())?
        .is_some()
    {
        bail!(TypedError::value_exists("numeric target id", target_id));
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
        [target_id],
    )?;

    Ok(())
}

/// Inserts a new storage target which may not exist yet.
///
/// Providing 0 for `target_id` chooses the ID automatically.
///
/// # Return value
/// Returns the ID of the new target.
pub(crate) fn insert_storage(
    tx: &Transaction,
    target_id: TargetId,
    alias: Option<Alias>,
) -> Result<TargetId> {
    let target_id = if target_id == 0 {
        misc::find_new_id(tx, "targets", "target_id", NodeType::Storage, 1..=0xFFFF)?
    } else if try_resolve_num_id(tx, EntityType::Target, NodeType::Storage, target_id.into())?
        .is_some()
    {
        return Ok(target_id);
    } else {
        target_id
    };

    insert(tx, target_id, alias, NodeTypeServer::Storage, None)?;

    Ok(target_id)
}

fn insert(
    tx: &Transaction,
    target_id: TargetId,
    alias: Option<Alias>,
    node_type: NodeTypeServer,
    // This is optional because storage targets come "unmapped"
    node_id: Option<NodeId>,
) -> Result<()> {
    let alias = if let Some(alias) = alias {
        alias
    } else {
        format!("target_{}_{}", node_type.user_str(), target_id).try_into()?
    };

    let new_uid = entity::insert(tx, EntityType::Target, &alias)?;

    tx.execute(
        sql!(
            "INSERT INTO targets (target_uid, node_type, target_id, node_id, pool_id)
            VALUES (?1, ?2, ?3, ?4, ?5)"
        ),
        params![
            new_uid,
            node_type.sql_variant(),
            target_id,
            node_id,
            if node_type == NodeTypeServer::Storage {
                Some(1)
            } else {
                None
            }
        ],
    )?;

    Ok(())
}

/// Changes the consistency state for the given targets to new individual values.
///
/// # Return value
/// Returns the number of affected entries.
pub(crate) fn update_consistency_states(
    tx: &Transaction,
    changes: impl IntoIterator<Item = (TargetId, TargetConsistencyState)>,
    node_type: NodeTypeServer,
) -> Result<usize> {
    let mut update = tx.prepare_cached(sql!(
        "UPDATE targets SET consistency = ?3
        WHERE consistency != ?3 AND target_uid = (
            SELECT target_uid FROM targets WHERE target_id = ?1 AND node_type = ?2
        )"
    ))?;

    let mut updated = 0;
    for e in changes {
        updated += update.execute(params![e.0, node_type.sql_variant(), e.1.sql_variant()])?;
    }

    Ok(updated)
}

/// Change the storage pool of the given targets IDs to a new one.
pub(crate) fn update_storage_pools(
    tx: &Transaction,
    new_pool_id: PoolId,
    target_ids: &[TargetId],
) -> Result<()> {
    let _ = resolve_num_id(tx, EntityType::Pool, NodeType::Storage, new_pool_id.into())?;

    validate_ids(tx, target_ids, NodeTypeServer::Storage)?;

    tx.execute(
        sql!("UPDATE targets SET pool_id = ?1 WHERE target_id IN rarray(?2) AND node_type = ?3"),
        params![
            new_pool_id,
            &rarray_param(target_ids.iter().copied()),
            NodeType::Storage.sql_variant()
        ],
    )?;

    Ok(())
}

/// Assigns the given storage targets to a new node.
///
/// # Return value
/// Returns the number of affected entries
pub(crate) fn update_storage_node_mappings(
    tx: &Transaction,
    target_ids: &[TargetId],
    new_node_id: NodeId,
) -> Result<usize> {
    let mut stmt = tx.prepare_cached(sql!(
        "UPDATE targets SET node_id = ?1 WHERE target_id = ?2 AND node_type = ?3"
    ))?;

    let mut updated = 0;
    for target_id in target_ids {
        updated += stmt.execute(params![
            new_node_id,
            target_id,
            NodeType::Storage.sql_variant()
        ])?;
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
    tx: &Transaction,
    items: impl IntoIterator<Item = Result<(TargetId, TargetCapacities)>>,
    node_type: NodeTypeServer,
) -> Result<Vec<(TargetId, TargetCapacities)>> {
    let mut select = tx.prepare_cached(sql!(
        "SELECT total_space, total_inodes, free_space, free_inodes
        FROM targets_ext
        WHERE target_id = ?1 AND node_type = ?2;"
    ))?;

    let mut update = tx.prepare_cached(sql!(
        "UPDATE targets
        SET total_space = ?1, total_inodes = ?2, free_space = ?3, free_inodes = ?4
        WHERE target_uid = (
            SELECT target_uid FROM targets WHERE target_id = ?5 AND node_type = ?6
        )"
    ))?;

    let mut old_values = vec![];

    for i in items {
        let i = i?;

        old_values.push(
            select.query_row(params![i.0, node_type.sql_variant()], |row| {
                Ok((
                    i.0,
                    TargetCapacities {
                        total_space: row.get(0)?,
                        total_inodes: row.get(1)?,
                        free_space: row.get(2)?,
                        free_inodes: row.get(3)?,
                    },
                ))
            })?,
        );

        update.execute(params![
            i.1.total_space,
            i.1.total_inodes,
            i.1.free_space,
            i.1.free_inodes,
            i.0,
            node_type.sql_variant()
        ])?;
    }

    Ok(old_values)
}

/// Deletes a storage target.
pub(crate) fn delete_storage(tx: &Transaction, target_id: TargetId) -> Result<()> {
    let affected = tx.execute_cached(
        sql!("DELETE FROM targets WHERE target_id = ?1 AND node_type = ?2"),
        params![target_id, NodeType::Storage.sql_variant()],
    )?;

    check_affected_rows(affected, [1])
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn set_get_meta() {
        with_test_data(|tx| {
            super::insert_meta(tx, 1, Some("existing_meta_target".try_into().unwrap()))
                .unwrap_err();
            super::insert_meta(tx, 99, Some("new_meta_target".try_into().unwrap())).unwrap();
            // existing alias
            super::insert_meta(tx, 99, Some("new_meta_target".try_into().unwrap())).unwrap_err();

            let targets: i64 = tx
                .query_row(sql!("SELECT COUNT(*) FROM meta_targets"), [], |row| {
                    row.get(0)
                })
                .unwrap();

            assert_eq!(5, targets);
        })
    }

    #[test]
    fn set_get_storage_and_map() {
        with_test_data(|tx| {
            let new_target_id =
                super::insert_storage(tx, 0, Some("new_storage_target".try_into().unwrap()))
                    .unwrap();
            super::insert_storage(tx, 1000, Some("new_storage_target_2".try_into().unwrap()))
                .unwrap();

            // existing alias
            super::insert_storage(tx, 0, Some("new_storage_target".try_into().unwrap()))
                .unwrap_err();

            super::update_storage_node_mappings(tx, &[new_target_id, 1000], 1).unwrap();

            assert_eq!(
                1,
                super::update_storage_node_mappings(tx, &[9999, 1], 1).unwrap()
            );

            let targets: Vec<TargetId> = tx
                .query_map_collect(sql!("SELECT target_id FROM storage_targets"), [], |row| {
                    row.get(0)
                })
                .unwrap();

            assert_eq!(19, targets.len());

            assert!(targets.iter().any(|e| *e == new_target_id));
            assert!(targets.iter().any(|e| *e == 1000));
        })
    }
}
