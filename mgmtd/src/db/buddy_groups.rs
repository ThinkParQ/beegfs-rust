use super::*;
use std::time::Duration;

#[derive(Clone, Debug)]
#[allow(dead_code)]
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

pub fn with_type(tx: &mut Transaction, node_type: NodeTypeServer) -> Result<Vec<BuddyGroup>> {
    let mut stmt = tx.prepare_cached(
        r#"
        SELECT
            buddy_group_id, primary_node_id, secondary_node_id,
            primary_target_id, secondary_target_id, pool_id,
            primary_free_space, primary_free_inodes,
            secondary_free_space, secondary_free_inodes
        FROM all_buddy_groups_v
        WHERE node_type = ?1;
        "#,
    )?;

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

pub fn insert(
    tx: &mut Transaction,
    id: Option<BuddyGroupID>,
    node_type: NodeTypeServer,
    primary_target_id: TargetID,
    secondary_target_id: TargetID,
) -> Result<BuddyGroupID> {
    let new_id = if let Some(id) = id {
        id
    } else {
        misc::find_new_id(
            tx,
            &format!("{}_buddy_groups", node_type.as_sql_str()),
            "buddy_group_id",
            1..=0xFFFF,
        )?
        .into()
    };

    let mut stmt = tx.prepare_cached(
        r#"
        INSERT INTO entities (entity_type, alias)
        VALUES ("buddy_group", ?1)
        "#,
    )?;

    stmt.execute(params![format!("{node_type}_buddy_group_{new_id}")])?;

    let new_uid: BuddyGroupUID = tx.last_insert_rowid().into();

    tx.execute(
        r#"
        INSERT INTO buddy_groups (buddy_group_uid, node_type) VALUES (?1, ?2)
        "#,
        params![new_uid, node_type],
    )?;

    tx.execute(
        &format!(
            r#"
            INSERT INTO {}_buddy_groups
            (buddy_group_id, buddy_group_uid, primary_target_id, secondary_target_id {})
            VALUES (?1, ?2, ?3, ?4 {})
            "#,
            node_type.as_sql_str(),
            if node_type == NodeTypeServer::Storage {
                ", pool_id"
            } else {
                ""
            },
            if node_type == NodeTypeServer::Storage {
                ", (SELECT pool_id FROM storage_targets WHERE target_id = ?3)"
            } else {
                ""
            },
        ),
        params![new_id, new_uid, primary_target_id, secondary_target_id],
    )?;

    Ok(new_id)
}

pub fn update_storage_pools(
    tx: &mut Transaction,
    new_pool_id: StoragePoolID,
    move_ids: impl IntoIterator<Item = BuddyGroupID>,
) -> Result<()> {
    let mut stmt = tx.prepare_cached(
        r#"
        UPDATE storage_buddy_groups SET pool_id = ?1 WHERE buddy_group_id = ?2
        "#,
    )?;

    for id in move_ids {
        let affected = stmt.execute(params![new_pool_id, id])?;
        ensure_rows_modified!(affected, id);
    }

    Ok(())
}

pub fn check_and_swap_buddies(
    tx: &mut Transaction,
    timeout: Duration,
) -> Result<Vec<(BuddyGroupID, NodeTypeServer)>> {
    // TODO use constants for timeout?
    let mut select = tx.prepare_cached(
        r#"
        SELECT buddy_group_id, node_type FROM all_buddy_groups_v
        WHERE (
            primary_last_contact_s >= ?1
            OR
            primary_consistency == "needs_resync"
        )
        AND secondary_consistency == "good"
        AND secondary_last_contact_s < (?1 / 2)
        "#,
    )?;

    let mut update_meta = tx.prepare_cached(
        r#"
        UPDATE meta_buddy_groups
        SET primary_target_id = secondary_target_id, secondary_target_id = primary_target_id
        WHERE buddy_group_id = ?1
        "#,
    )?;

    let mut update_storage = tx.prepare_cached(
        r#"
        UPDATE storage_buddy_groups
        SET primary_target_id = secondary_target_id, secondary_target_id = primary_target_id
        WHERE buddy_group_id = ?1
        "#,
    )?;

    let mut rows = select.query([timeout.as_secs()])?;

    let mut affected = vec![];
    while let Some(row) = rows.next()? {
        let id: BuddyGroupID = row.get(0)?;
        let node_type: NodeTypeServer = row.get(1)?;

        match node_type {
            NodeTypeServer::Meta => update_meta.execute([id])?,
            NodeTypeServer::Storage => update_storage.execute([id])?,
        };

        affected.push((id, node_type));
    }

    Ok(affected)
}

pub fn prepare_storage_deletion(
    tx: &mut Transaction,
    id: BuddyGroupID,
) -> Result<(NodeUID, NodeUID)> {
    let mut stmt = tx.prepare_cached(
        r#"
        SELECT COUNT(*) FROM client_nodes
        "#,
    )?;

    if 0 < stmt.query_row([], |row| row.get::<_, i32>(0))? {
        bail!("Can't remove storage buddy while clients are still mounted");
    }

    let mut stmt = tx.prepare_cached(
        r#"
        SELECT psn.node_uid, ssn.node_uid
        FROM storage_buddy_groups AS g
        INNER JOIN storage_targets AS pst ON pst.target_id = g.primary_target_id
        INNER JOIN storage_nodes AS psn ON psn.node_id = pst.node_id
        INNER JOIN storage_targets AS sst ON sst.target_id = g.secondary_target_id
        INNER JOIN storage_nodes AS ssn ON ssn.node_id = sst.node_id
        WHERE buddy_group_id = ?1;
        "#,
    )?;

    let node_uids = stmt.query_row([id], |row| Ok((row.get(0)?, row.get(1)?)))?;

    Ok(node_uids)
}

pub fn delete_storage(tx: &mut Transaction, buddy_group_id: BuddyGroupID) -> Result<()> {
    let affected = tx.execute(
        r#"
        DELETE FROM storage_buddy_groups WHERE buddy_group_id = ?1
        "#,
        [buddy_group_id],
    )?;
    ensure_rows_modified!(affected, buddy_group_id);

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::db::test::*;

    /// Test inserting and getting buddy groups
    #[test]
    fn insert_and_get_with_type() {
        with_test_data(|tx| {
            super::insert(
                tx,
                Some(1234.into()),
                NodeTypeServer::Meta,
                3.into(),
                4.into(),
            )
            .unwrap();
            super::insert(tx, None, NodeTypeServer::Storage, 2.into(), 6.into()).unwrap();

            super::insert(tx, None, NodeTypeServer::Meta, 5.into(), 6.into()).unwrap_err();
            super::insert(
                tx,
                Some(1.into()),
                NodeTypeServer::Storage,
                3.into(),
                7.into(),
            )
            .unwrap_err();

            let meta_groups = super::with_type(tx, NodeTypeServer::Meta).unwrap();
            let storage_groups = super::with_type(tx, NodeTypeServer::Storage).unwrap();

            assert_eq!(2, meta_groups.len());
            assert_eq!(3, storage_groups.len());
            assert!(meta_groups.iter().any(|e| e.id == 1234.into()));
        })
    }

    /// Test updating the storage pool of a buddy group
    #[test]
    fn update_storage_pool() {
        with_test_data(|tx| {
            super::update_storage_pools(tx, 2.into(), [1.into()]).unwrap();
            super::update_storage_pools(tx, 2.into(), [99.into()]).unwrap_err();
            super::update_storage_pools(tx, 99.into(), [1.into()]).unwrap_err();

            let storage_groups = super::with_type(tx, NodeTypeServer::Storage).unwrap();

            assert_eq!(
                Some(2.into()),
                storage_groups
                    .iter()
                    .find(|e| e.id == 1.into())
                    .unwrap()
                    .pool_id
            );
        })
    }

    /// Makes sure targets of buddy groups 1 (meta and storage) have been swapped
    fn ensure_swapped_buddies(tx: &mut Transaction) {
        let meta_groups = super::with_type(tx, NodeTypeServer::Meta).unwrap();
        let storage_groups = super::with_type(tx, NodeTypeServer::Storage).unwrap();

        assert_eq!(TargetID::from(2), meta_groups[0].primary_target_id);
        assert_eq!(TargetID::from(1), meta_groups[0].secondary_target_id);
        assert_eq!(TargetID::from(5), storage_groups[0].primary_target_id);
        assert_eq!(TargetID::from(1), storage_groups[0].secondary_target_id);
    }

    /// Makes sure targets of buddy groups 1 (meta and storage) have not been swapped
    fn ensure_no_swapped_buddies(tx: &mut Transaction) {
        let meta_groups = super::with_type(tx, NodeTypeServer::Meta).unwrap();
        let storage_groups = super::with_type(tx, NodeTypeServer::Storage).unwrap();

        assert_eq!(TargetID::from(1), meta_groups[0].primary_target_id);
        assert_eq!(TargetID::from(2), meta_groups[0].secondary_target_id);
        assert_eq!(TargetID::from(1), storage_groups[0].primary_target_id);
        assert_eq!(TargetID::from(5), storage_groups[0].secondary_target_id);
    }

    /// Test swapping primary and secondary member (switchover) when primary is needs_resync
    #[test]
    fn swap_buddies_on_needs_resync() {
        with_test_data(|tx| {
            targets::update_consistency_states(
                tx,
                [(TargetID::from(1), TargetConsistencyState::NeedsResync)],
                NodeTypeServer::Meta,
            )
            .unwrap();

            targets::update_consistency_states(
                tx,
                [(TargetID::from(1), TargetConsistencyState::NeedsResync)],
                NodeTypeServer::Storage,
            )
            .unwrap();

            let swaps = super::check_and_swap_buddies(tx, Duration::from_secs(10000)).unwrap();

            assert_eq!(2, swaps.len());
            assert!(swaps
                .iter()
                .any(|e| e.0 == 1.into() && e.1 == NodeTypeServer::Meta));
            assert!(swaps
                .iter()
                .any(|e| e.0 == 1.into() && e.1 == NodeTypeServer::Storage));

            ensure_swapped_buddies(tx);

            assert!(
                super::check_and_swap_buddies(tx, Duration::from_secs(99999))
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
                .any(|e| e.0 == 1.into() && e.1 == NodeTypeServer::Meta));
            assert!(swaps
                .iter()
                .any(|e| e.0 == 1.into() && e.1 == NodeTypeServer::Storage));

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
            targets::update_consistency_states(
                tx,
                [
                    (TargetID::from(1), TargetConsistencyState::NeedsResync),
                    (TargetID::from(2), TargetConsistencyState::NeedsResync),
                ],
                NodeTypeServer::Meta,
            )
            .unwrap();

            targets::update_consistency_states(
                tx,
                [
                    (TargetID::from(1), TargetConsistencyState::NeedsResync),
                    (TargetID::from(5), TargetConsistencyState::NeedsResync),
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
            super::prepare_storage_deletion(tx, 1.into()).unwrap_err();
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

            let res = super::prepare_storage_deletion(tx, 1.into()).unwrap();

            assert_eq!((NodeUID::from(102001), NodeUID::from(102002)), res);
        })
    }

    #[test]
    fn delete_storage() {
        with_test_data(|tx| {
            super::delete_storage(tx, 1.into()).unwrap();

            let groups = super::with_type(tx, NodeTypeServer::Storage).unwrap();
            assert_eq!(1, groups.len());
        })
    }
}
