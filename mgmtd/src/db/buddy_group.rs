//! Functions for buddy group management

use super::*;
use itertools::Itertools;
use std::cmp::Ordering;
use std::time::Duration;

/// Represents a buddy group entry.
#[derive(Clone, Debug)]
pub(crate) struct BuddyGroup {
    pub id: BuddyGroupId,
    pub primary_target_id: TargetId,
    pub secondary_target_id: TargetId,
    #[allow(unused)]
    pub pool_id: Option<PoolId>,
}

/// Retrieve a list of nodes filtered by node type.
pub(crate) fn get_with_type(
    tx: &Transaction,
    node_type: NodeTypeServer,
) -> Result<Vec<BuddyGroup>> {
    Ok(tx.query_map_collect(
        sql!(
            "SELECT group_id, p_target_id, s_target_id, pool_id
            FROM all_buddy_groups_v
            WHERE node_type = ?1;"
        ),
        [node_type.sql_str()],
        |row| {
            Ok(BuddyGroup {
                id: row.get(0)?,
                primary_target_id: row.get(1)?,
                secondary_target_id: row.get(2)?,
                pool_id: row.get(3)?,
            })
        },
    )?)
}

/// Ensures that the list of given buddy groups actually exists and returns an appropriate error if
/// not.
pub(crate) fn validate_ids(
    tx: &Transaction,
    group_ids: &[BuddyGroupId],
    node_type: NodeTypeServer,
) -> Result<()> {
    let stmt = match node_type {
        NodeTypeServer::Meta => {
            sql!("SELECT COUNT(*) FROM meta_buddy_groups WHERE group_id IN rarray(?1)")
        }
        NodeTypeServer::Storage => {
            sql!("SELECT COUNT(*) FROM storage_buddy_groups WHERE group_id IN rarray(?1)")
        }
    };

    let count: usize =
        tx.query_row_cached(stmt, [&rarray_param(group_ids.iter().copied())], |row| {
            row.get(0)
        })?;

    match count.cmp(&group_ids.len()) {
        Ordering::Less => Err(anyhow!(TypedError::value_not_found(
            "numeric buddy group id",
            group_ids.iter().join(", "),
        ))),
        Ordering::Greater => Err(anyhow!(
            "Buddy group ids have multiple matches: {} provided, {} selected",
            group_ids.len(),
            count
        )),
        Ordering::Equal => Ok(()),
    }
}

/// Inserts a new buddy group.
///
/// Providing 0 for `group_id` chooses the ID automatically.
///
/// # Return value
/// Returns the ID of the new buddy group.
pub(crate) fn insert(
    tx: &Transaction,
    group_id: BuddyGroupId,
    alias: &Alias,
    node_type: NodeTypeServer,
    p_target_id: TargetId,
    s_target_id: TargetId,
) -> Result<(Uid, BuddyGroupId)> {
    let group_id = if group_id == 0 {
        misc::find_new_id(
            tx,
            &format!("{}_buddy_groups", node_type.sql_str()),
            "group_id",
            1..=0xFFFF,
        )?
    } else if try_resolve_num_id(
        tx,
        EntityType::BuddyGroup,
        node_type.into(),
        group_id.into(),
    )?
    .is_some()
    {
        bail!(TypedError::value_exists("numeric buddy group id", group_id));
    } else {
        group_id
    };

    // Check targets exist
    target::validate_ids(tx, &[p_target_id, s_target_id], node_type)?;

    // Check that both given target IDs are not assigned to any buddy group
    if tx.query_row(
        sql!(
            "SELECT COUNT(*) FROM all_buddy_groups_v
             WHERE node_type = ?1
             AND (p_target_id IN (?2, ?3) OR s_target_id IN (?2, ?3))"
        ),
        params![node_type.sql_str(), p_target_id, s_target_id],
        |row| row.get::<_, i64>(0),
    )? > 0
    {
        bail!(
            "One or both of the given numeric target ids {p_target_id} and {s_target_id} \
             are already part of a buddy group"
        );
    }

    // Check that both targets are part of the same storage pool
    if node_type == NodeTypeServer::Storage {
        let mut check = tx.prepare(sql!(
            "SELECT pool_id FROM storage_targets WHERE target_id = ?1"
        ))?;

        let p_pool_id: PoolId = check.query_row([p_target_id], |row| row.get(0))?;
        let s_pool_id: PoolId = check.query_row([s_target_id], |row| row.get(0))?;

        if p_pool_id != s_pool_id {
            bail!("Primary and secondary target are not assigned to the same storage pool");
        }
    }

    // Insert entity
    let new_uid = entity::insert(tx, EntityType::BuddyGroup, alias)?;

    // Insert generic buddy group
    tx.execute(
        sql!("INSERT INTO buddy_groups (group_uid, node_type) VALUES (?1, ?2)"),
        params![new_uid, node_type.sql_str()],
    )?;

    // Insert type specific buddy group
    tx.execute(
        match node_type {
            NodeTypeServer::Meta => {
                sql!(
                    "INSERT INTO meta_buddy_groups
                    (group_id, group_uid, p_target_id, s_target_id)
                    VALUES (?1, ?2, ?3, ?4)"
                )
            }
            NodeTypeServer::Storage => {
                sql!(
                    "INSERT INTO storage_buddy_groups
                    (group_id, group_uid, p_target_id, s_target_id, pool_id)
                    VALUES (?1, ?2, ?3, ?4,
                        (SELECT pool_id FROM storage_targets WHERE target_id = ?3)
                    )"
                )
            }
        },
        params![group_id, new_uid, p_target_id, s_target_id],
    )?;

    Ok((new_uid, group_id))
}

/// Changes the storage pool of the given buddy group IDs to a new one.
pub(crate) fn update_storage_pools(
    tx: &Transaction,
    new_pool_id: PoolId,
    group_ids: &[BuddyGroupId],
) -> Result<()> {
    let _ = resolve_num_id(tx, EntityType::Pool, NodeType::Storage, new_pool_id.into())?;

    validate_ids(tx, group_ids, NodeTypeServer::Storage)?;

    tx.execute_cached(
        sql!("UPDATE storage_buddy_groups SET pool_id = ?1 WHERE group_id IN rarray(?2)"),
        params![new_pool_id, rarray_param(group_ids.iter().copied())],
    )?;

    Ok(())
}

/// Checks all buddy groups state and swaps primary and secondary if necessary ("switchover").
///
/// This, of course, only affectes the database, the new state has to be broadcast to the affected
/// nodes from the caller.
///
/// # Conditions for a swap
/// A swap happens, if
/// * primaries last contact was more than `timeout` ago OR primaries consistency state is
///   `needs_resync`
/// * AND secondaries consistency state is `good`
/// * AND secondaries last contact was less than `timeout / 2` ago
///
/// # Return value
/// Returns a Vec containing tuples with the ID and the node type of buddy groups which have been
/// swapped.
pub(crate) fn check_and_swap_buddies(
    tx: &Transaction,
    timeout: Duration,
) -> Result<Vec<(BuddyGroupId, NodeTypeServer)>> {
    let affected_groups = tx.query_map_collect(
        sql!(
            "SELECT g.group_id, g.node_type FROM all_buddy_groups_v AS g
            INNER JOIN all_targets_v AS p_t ON p_t.target_uid = p_target_uid
            INNER JOIN nodes AS p_n ON p_n.node_uid = p_t.node_uid
            INNER JOIN all_targets_v AS s_t ON s_t.target_uid = s_target_uid
            INNER JOIN nodes AS s_n ON s_n.node_uid = s_t.node_uid
            WHERE (
                (STRFTIME('%s', 'now') - STRFTIME('%s', p_n.last_contact)) >= ?1
                OR
                p_t.consistency == 'needs_resync'
            )
                AND s_t.consistency == 'good'
                AND (STRFTIME('%s', 'now') - STRFTIME('%s', s_n.last_contact)) < (?1 / 2)"
        ),
        [timeout.as_secs()],
        |row| Ok((row.get(0)?, NodeTypeServer::from_row(row, 1)?)),
    )?;

    for &(id, node_type) in &affected_groups {
        let affected = tx.execute(
            match node_type {
                NodeTypeServer::Meta => sql!(
                    "UPDATE meta_buddy_groups
                    SET p_target_id = s_target_id, s_target_id = p_target_id
                    WHERE group_id = ?1"
                ),
                NodeTypeServer::Storage => sql!(
                    "UPDATE storage_buddy_groups
                    SET p_target_id = s_target_id, s_target_id = p_target_id
                    WHERE group_id = ?1"
                ),
            },
            [id],
        )?;

        check_affected_rows(affected, [1])?;
    }

    Ok(affected_groups)
}

/// Checks if a storage buddy group can be deleted and provides necessary information.
///
/// # Return value
/// Returns the UIDs of the primary and the secondary node which own the primary and secondary
/// target of the given group.
pub(crate) fn prepare_storage_deletion(tx: &Transaction, id: BuddyGroupId) -> Result<(Uid, Uid)> {
    if tx.query_row(sql!("SELECT COUNT(*) FROM client_nodes"), [], |row| {
        row.get::<_, i64>(0)
    })? > 0
    {
        bail!("Can't remove storage buddy group while clients are still mounted",);
    }

    let node_uids = tx.query_row(
        sql!(
            "SELECT p_sn.node_uid, s_sn.node_uid
            FROM storage_buddy_groups AS g
            INNER JOIN storage_targets AS p_st ON p_st.target_id = g.p_target_id
            INNER JOIN storage_nodes AS p_sn ON p_sn.node_id = p_st.node_id
            INNER JOIN storage_targets AS s_st ON s_st.target_id = g.s_target_id
            INNER JOIN storage_nodes AS s_sn ON s_sn.node_id = s_st.node_id
            WHERE group_id = ?1;"
        ),
        [id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    Ok(node_uids)
}

/// Deletes a storage buddy group.
///
/// This expects that the nodes owning the affected targets have already been notified and the
/// groups deleted.
pub(crate) fn delete_storage(tx: &Transaction, group_id: BuddyGroupId) -> Result<()> {
    let affected = tx.execute(
        sql!("DELETE FROM storage_buddy_groups WHERE group_id = ?1"),
        [group_id],
    )?;

    check_affected_rows(affected, [1])
}

#[cfg(test)]
mod test {
    use super::*;

    /// Test inserting and getting buddy groups
    #[test]
    fn insert_and_get_with_type() {
        with_test_data(|tx| {
            super::insert(
                tx,
                1234,
                &"g1".try_into().unwrap(),
                NodeTypeServer::Meta,
                3,
                4,
            )
            .unwrap();
            super::insert(
                tx,
                1,
                &"g2".try_into().unwrap(),
                NodeTypeServer::Storage,
                3,
                7,
            )
            .unwrap_err();

            let meta_groups = super::get_with_type(tx, NodeTypeServer::Meta).unwrap();
            let storage_groups = super::get_with_type(tx, NodeTypeServer::Storage).unwrap();

            assert_eq!(2, meta_groups.len());
            assert_eq!(2, storage_groups.len());
            assert!(meta_groups.iter().any(|e| e.id == 1234));
        })
    }

    /// Test updating the storage pool of a buddy group
    #[test]
    fn update_storage_pool() {
        with_test_data(|tx| {
            super::update_storage_pools(tx, 2, &[1]).unwrap();
            super::update_storage_pools(tx, 99, &[1]).unwrap_err();

            let storage_groups = super::get_with_type(tx, NodeTypeServer::Storage).unwrap();

            assert_eq!(
                Some(2),
                storage_groups.iter().find(|e| e.id == 1).unwrap().pool_id
            );
        })
    }

    /// Makes sure targets of buddy groups 1 (meta and storage) have been swapped
    fn ensure_swapped_buddies(tx: &Transaction) {
        let meta_groups = super::get_with_type(tx, NodeTypeServer::Meta).unwrap();
        let storage_groups = super::get_with_type(tx, NodeTypeServer::Storage).unwrap();

        assert_eq!(2, meta_groups[0].primary_target_id);
        assert_eq!(1, meta_groups[0].secondary_target_id);
        assert_eq!(5, storage_groups[0].primary_target_id);
        assert_eq!(1, storage_groups[0].secondary_target_id);
    }

    /// Makes sure targets of buddy groups 1 (meta and storage) have not been swapped
    fn ensure_no_swapped_buddies(tx: &Transaction) {
        let meta_groups = super::get_with_type(tx, NodeTypeServer::Meta).unwrap();
        let storage_groups = super::get_with_type(tx, NodeTypeServer::Storage).unwrap();

        assert_eq!(1, meta_groups[0].primary_target_id);
        assert_eq!(2, meta_groups[0].secondary_target_id);
        assert_eq!(1, storage_groups[0].primary_target_id);
        assert_eq!(5, storage_groups[0].secondary_target_id);
    }

    /// Test swapping primary and secondary member (switchover) when primary is needs_resync
    #[test]
    fn swap_buddies_on_needs_resync() {
        with_test_data(|tx| {
            target::update_consistency_states(
                tx,
                [(1, TargetConsistencyState::NeedsResync)],
                NodeTypeServer::Meta,
            )
            .unwrap();

            target::update_consistency_states(
                tx,
                [(1, TargetConsistencyState::NeedsResync)],
                NodeTypeServer::Storage,
            )
            .unwrap();

            let swaps = super::check_and_swap_buddies(tx, Duration::from_secs(10000)).unwrap();

            assert_eq!(2, swaps.len());
            assert!(swaps
                .iter()
                .any(|e| e.0 == 1 && e.1 == NodeTypeServer::Meta));
            assert!(swaps
                .iter()
                .any(|e| e.0 == 1 && e.1 == NodeTypeServer::Storage));

            ensure_swapped_buddies(tx);

            assert!(
                buddy_group::check_and_swap_buddies(tx, Duration::from_secs(99999))
                    .unwrap()
                    .is_empty()
            );
        })
    }

    /// Test swapping primary and secondary member (switchover) when primary runs into timeout
    #[test]
    fn swap_buddies_on_timeout() {
        with_test_data(|tx| {
            tx.execute(
                "UPDATE nodes
                SET last_contact = DATETIME('now', '-1 hour')
                WHERE node_uid IN (101001, 102001)",
                [],
            )
            .unwrap();

            let swaps = super::check_and_swap_buddies(tx, Duration::from_secs(100)).unwrap();

            assert_eq!(2, swaps.len());
            assert!(swaps
                .iter()
                .any(|e| e.0 == 1 && e.1 == NodeTypeServer::Meta));
            assert!(swaps
                .iter()
                .any(|e| e.0 == 1 && e.1 == NodeTypeServer::Storage));

            ensure_swapped_buddies(tx);
        })
    }

    /// Test that buddies are not swapped if secodary doesn't satisfy the conditions
    #[test]
    fn no_swap_buddies_on_secondary_timeout() {
        with_test_data(|tx| {
            // Trigger timeout for all buddy nodes (including secondaries). This should not cause a
            // switchover
            tx.execute(
                "UPDATE nodes
                SET last_contact = DATETIME('now', '-1 hour')
                WHERE node_uid IN (101001, 101002, 102001, 102002)",
                [],
            )
            .unwrap();

            super::check_and_swap_buddies(tx, Duration::from_secs(99999)).unwrap();

            ensure_no_swapped_buddies(tx);
        })
    }

    #[test]
    fn no_swap_buddies_on_secondary_needs_resync() {
        with_test_data(|tx| {
            target::update_consistency_states(
                tx,
                [
                    (1, TargetConsistencyState::NeedsResync),
                    (2, TargetConsistencyState::NeedsResync),
                ],
                NodeTypeServer::Meta,
            )
            .unwrap();

            target::update_consistency_states(
                tx,
                [
                    (1, TargetConsistencyState::NeedsResync),
                    (5, TargetConsistencyState::NeedsResync),
                ],
                NodeTypeServer::Storage,
            )
            .unwrap();

            super::check_and_swap_buddies(tx, Duration::from_secs(99999)).unwrap();

            ensure_no_swapped_buddies(tx);
        })
    }

    #[test]
    fn mounted_clients_fail_prepare_storage_deletion() {
        with_test_data(|tx| {
            super::prepare_storage_deletion(tx, 1).unwrap_err();
        })
    }

    #[test]
    fn prepare_storage_deletion_returns_correct_node_uids() {
        with_test_data(|tx| {
            tx.execute("DELETE FROM nodes WHERE node_type = 'client'", [])
                .unwrap();

            let res = super::prepare_storage_deletion(tx, 1).unwrap();

            assert_eq!((Uid::from(102001i64), Uid::from(102002i64)), res);
        })
    }

    #[test]
    fn delete_storage() {
        with_test_data(|tx| {
            super::delete_storage(tx, 1).unwrap();

            let groups = super::get_with_type(tx, NodeTypeServer::Storage).unwrap();
            assert_eq!(1, groups.len());
        })
    }
}
