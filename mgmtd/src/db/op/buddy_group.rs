//! Functions for buddy group management

use super::*;
use itertools::Itertools;
use std::cmp::Ordering;
use std::time::Duration;

/// Represents a buddy group entry.
#[derive(Clone, Debug)]
pub struct BuddyGroup {
    pub id: BuddyGroupID,
    pub primary_node_id: NodeID,
    pub secondary_node_id: NodeID,
    pub primary_target_id: TargetID,
    pub secondary_target_id: TargetID,
    pub pool_id: Option<StoragePoolID>,
    pub primary_free_space: Option<u64>,
    pub primary_free_inodes: Option<u64>,
    pub secondary_free_space: Option<u64>,
    pub secondary_free_inodes: Option<u64>,
}
/// Retrieve a list of nodes filtered by node type.
pub fn get_with_type(tx: &mut Transaction, node_type: NodeTypeServer) -> Result<Vec<BuddyGroup>> {
    let mut stmt = tx.prepare_cached(sql!(
        r#"
        SELECT
            buddy_group_id, primary_node_id, secondary_node_id,
            primary_target_id, secondary_target_id, pool_id,
            primary_free_space, primary_free_inodes,
            secondary_free_space, secondary_free_inodes
        FROM all_buddy_groups_v
        WHERE node_type = ?1;
        "#
    ))?;

    let res = stmt
        .query_map([node_type], |row| {
            Ok(BuddyGroup {
                id: row.get(0)?,
                primary_node_id: row.get(1)?,
                secondary_node_id: row.get(2)?,
                primary_target_id: row.get(3)?,
                secondary_target_id: row.get(4)?,
                pool_id: row.get(5)?,
                primary_free_space: row.get(6)?,
                primary_free_inodes: row.get(7)?,
                secondary_free_space: row.get(8)?,
                secondary_free_inodes: row.get(9)?,
            })
        })?
        .try_collect()?;

    Ok(res)
}

/// Retrieve the global UID for the given buddy group ID and type.
///
/// # Return value
/// Returns `None` if the entry doesn't exist.
pub fn get_uid(
    tx: &mut Transaction,
    buddy_group_id: BuddyGroupID,
    node_type: NodeTypeServer,
) -> Result<Option<EntityUID>> {
    let res: Option<EntityUID> = tx
        .query_row_cached(
            sql!(
                "SELECT buddy_group_uid FROM all_buddy_groups_v WHERE buddy_group_id = ?1 AND \
                 node_type = ?2"
            ),
            params![buddy_group_id, node_type],
            |row| row.get(0),
        )
        .optional()?;

    Ok(res)
}

/// Ensures that the list of given buddy groups actually exists and returns an appropriate error if
/// not.
pub fn validate_ids(
    tx: &mut Transaction,
    buddy_group_ids: &[BuddyGroupID],
    node_type: NodeTypeServer,
) -> Result<()> {
    let count: usize = tx.query_row_cached(
        &format!(
            "SELECT COUNT(*) FROM {}_buddy_groups WHERE buddy_group_id IN ({}) ",
            node_type.as_sql_str(),
            buddy_group_ids.iter().join(",")
        ),
        [],
        |row| row.get(0),
    )?;

    match count.cmp(&buddy_group_ids.len()) {
        Ordering::Less => Err(anyhow!(TypedError::value_not_found(
            "buddy group ID",
            buddy_group_ids.iter().join(", "),
        ))),
        Ordering::Greater => Err(anyhow!(
            "Buddy group IDs have multiple matches: {} provided, {} selected",
            buddy_group_ids.len(),
            count
        )),
        Ordering::Equal => Ok(()),
    }
}

/// Inserts a new buddy group.
///
/// # Return value
/// Returns the ID of the new buddy group.
pub fn insert(
    tx: &mut Transaction,
    buddy_group_id: Option<BuddyGroupID>,
    node_type: NodeTypeServer,
    primary_target_id: TargetID,
    secondary_target_id: TargetID,
) -> Result<BuddyGroupID> {
    let new_id = if let Some(buddy_group_id) = buddy_group_id {
        buddy_group_id
    } else {
        misc::find_new_id(
            tx,
            &format!("{}_buddy_groups", node_type.as_sql_str()),
            "buddy_group_id",
            1..=0xFFFF,
        )?
    };

    // Check that both given target IDs are not assigned to any buddy group
    if tx.query_row(
        sql!(
            "SELECT COUNT(*) FROM all_buddy_groups_v
             WHERE node_type = ?1
             AND (primary_target_id IN (?2, ?3) OR secondary_target_id IN (?2, ?3))"
        ),
        params![node_type, primary_target_id, secondary_target_id],
        |row| row.get::<_, i64>(0),
    )? > 0
    {
        bail!(
            "One or both of the given target IDs {primary_target_id} and {secondary_target_id} \
             are already part of a buddy group"
        );
    }

    // Insert entity
    let new_uid = entity::insert(
        tx,
        EntityType::BuddyGroup,
        &format!("{}_buddy_group_{new_id}", node_type.as_sql_str()),
    )?;

    // Insert generic buddy group
    tx.execute(
        sql!("INSERT INTO buddy_groups (buddy_group_uid, node_type) VALUES (?1, ?2)"),
        params![new_uid, node_type],
    )?;

    // Insert type specific buddy group
    tx.execute(
        match node_type {
            NodeTypeServer::Meta => {
                sql!(
                    r#"
                    INSERT INTO meta_buddy_groups
                    (buddy_group_id, buddy_group_uid, primary_target_id, secondary_target_id)
                    VALUES (?1, ?2, ?3, ?4)
                    "#
                )
            }
            NodeTypeServer::Storage => {
                sql!(
                    r#"
                    INSERT INTO storage_buddy_groups
                    (buddy_group_id, buddy_group_uid, primary_target_id, secondary_target_id, pool_id)
                    VALUES (?1, ?2, ?3, ?4, (SELECT pool_id FROM storage_targets WHERE target_id = ?3))
                    "#
                )
            }
        },
        params![new_id, new_uid, primary_target_id, secondary_target_id],
    )?;

    Ok(new_id)
}

/// Changes the storage pool of the given buddy group IDs to a new one.
pub(crate) fn update_storage_pools(
    tx: &mut Transaction,
    new_pool_id: StoragePoolID,
    buddy_group_ids: &[BuddyGroupID],
) -> Result<()> {
    let mut stmt = tx.prepare_cached(sql!(
        "UPDATE storage_buddy_groups SET pool_id = ?1 WHERE buddy_group_id = ?2"
    ))?;

    let mut updated = 0;
    for id in buddy_group_ids {
        updated += stmt.execute(params![new_pool_id, id])?;
    }

    if updated != buddy_group_ids.len() {
        bail!(
            "At least one of the given buddy group IDs ({}) doesn't exist",
            buddy_group_ids.iter().join(",")
        );
    }

    Ok(())
}

/// Reset all storage buddy groups belonging to the given storage pool to the default pool.
pub(crate) fn reset_storage_pool(
    tx: &mut Transaction,
    pool_id: StoragePoolID,
) -> rusqlite::Result<usize> {
    tx.execute_cached(
        sql!("UPDATE storage_buddy_groups SET pool_id = 1 WHERE pool_id = ?"),
        [pool_id],
    )
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
pub fn check_and_swap_buddies(
    tx: &mut Transaction,
    timeout: Duration,
) -> Result<Vec<(BuddyGroupID, NodeTypeServer)>> {
    let affected_groups: Vec<(BuddyGroupID, NodeTypeServer)> = {
        let mut select = tx.prepare_cached(sql!(
            r#"
            SELECT buddy_group_id, node_type FROM all_buddy_groups_v
            WHERE (
                primary_last_contact_s >= ?1
                OR
                primary_consistency == "needs_resync"
            )
            AND secondary_consistency == "good"
            AND secondary_last_contact_s < (?1 / 2)
            "#
        ))?;

        let affected_groups = select
            .query_map([timeout.as_secs()], |row| Ok((row.get(0)?, row.get(1)?)))?
            .try_collect()?;

        #[allow(clippy::let_and_return)]
        affected_groups
    };

    for (id, node_type) in &affected_groups {
        tx.execute_checked(
            match node_type {
                NodeTypeServer::Meta => sql!(
                    r#"
                    UPDATE meta_buddy_groups
                    SET primary_target_id = secondary_target_id, secondary_target_id = primary_target_id
                    WHERE buddy_group_id = ?1
                    "#
                ),
                NodeTypeServer::Storage => sql!(
                    r#"
                    UPDATE storage_buddy_groups
                    SET primary_target_id = secondary_target_id, secondary_target_id = primary_target_id
                    WHERE buddy_group_id = ?1
                    "#
                )
            },
            [id],
            1..=1,
        )?;
    }

    Ok(affected_groups)
}

/// Checks if a storage buddy group can be deleted and provides necessary information.
///
/// # Return value
/// Returns the UIDs of the primary and the secondary node which own the primary and secondary
/// target of the given group.
pub fn prepare_storage_deletion(
    tx: &mut Transaction,
    id: BuddyGroupID,
) -> Result<(EntityUID, EntityUID)> {
    if tx.query_row(sql!("SELECT COUNT(*) FROM client_nodes"), [], |row| {
        row.get::<_, i64>(0)
    })? > 0
    {
        bail!("Can't remove storage buddy group while clients are still mounted",);
    }

    let node_uids = tx.query_row(
        sql!(
            r#"
            SELECT psn.node_uid, ssn.node_uid
            FROM storage_buddy_groups AS g
            INNER JOIN storage_targets AS pst ON pst.target_id = g.primary_target_id
            INNER JOIN storage_nodes AS psn ON psn.node_id = pst.node_id
            INNER JOIN storage_targets AS sst ON sst.target_id = g.secondary_target_id
            INNER JOIN storage_nodes AS ssn ON ssn.node_id = sst.node_id
            WHERE buddy_group_id = ?1;
            "#
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
pub fn delete_storage(tx: &mut Transaction, buddy_group_id: BuddyGroupID) -> Result<()> {
    tx.execute_checked(
        sql!("DELETE FROM storage_buddy_groups WHERE buddy_group_id = ?1"),
        [buddy_group_id],
        1..=1,
    )?;

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    /// Test inserting and getting buddy groups
    #[test]
    fn insert_and_get_with_type() {
        with_test_data(|tx| {
            super::insert(tx, Some(1234), NodeTypeServer::Meta, 3, 4).unwrap();
            super::insert(tx, None, NodeTypeServer::Storage, 2, 6).unwrap();

            super::insert(tx, None, NodeTypeServer::Meta, 5, 6).unwrap_err();
            super::insert(tx, Some(1), NodeTypeServer::Storage, 3, 7).unwrap_err();

            let meta_groups = super::get_with_type(tx, NodeTypeServer::Meta).unwrap();
            let storage_groups = super::get_with_type(tx, NodeTypeServer::Storage).unwrap();

            assert_eq!(2, meta_groups.len());
            assert_eq!(3, storage_groups.len());
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
    fn ensure_swapped_buddies(tx: &mut Transaction) {
        let meta_groups = super::get_with_type(tx, NodeTypeServer::Meta).unwrap();
        let storage_groups = super::get_with_type(tx, NodeTypeServer::Storage).unwrap();

        assert_eq!(2, meta_groups[0].primary_target_id);
        assert_eq!(1, meta_groups[0].secondary_target_id);
        assert_eq!(5, storage_groups[0].primary_target_id);
        assert_eq!(1, storage_groups[0].secondary_target_id);
    }

    /// Makes sure targets of buddy groups 1 (meta and storage) have not been swapped
    fn ensure_no_swapped_buddies(tx: &mut Transaction) {
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
                r#"
                UPDATE nodes
                SET last_contact = DATETIME("now", "-1 hour")
                WHERE node_uid IN (101001, 102001)
                "#,
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
                r#"
                UPDATE nodes
                SET last_contact = DATETIME("now", "-1 hour")
                WHERE node_uid IN (101001, 101002, 102001, 102002)
                "#,
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
            tx.execute(
                r#"
                DELETE FROM nodes WHERE node_type = "client"
                "#,
                [],
            )
            .unwrap();

            let res = super::prepare_storage_deletion(tx, 1).unwrap();

            assert_eq!((EntityUID::from(102001), EntityUID::from(102002)), res);
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
