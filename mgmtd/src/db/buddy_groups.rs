use super::*;
use std::time::Duration;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct BuddyGroup {
    pub id: BuddyGroupID,
    pub node_type: NodeTypeServer,
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

pub(crate) fn with_type(
    tx: &mut Transaction,
    node_type: NodeTypeServer,
) -> Result<Vec<BuddyGroup>> {
    let mut stmt = tx.prepare_cached(
        r#"
        SELECT 
            buddy_group_id, node_type, primary_node_id, secondary_node_id,
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
                node_type: row.get(1)?,
                primary_node_id: row.get(2)?,
                secondary_node_id: row.get(3)?,
                primary_target_id: row.get(4)?,
                secondary_target_id: row.get(5)?,
                pool_id: row.get(6)?,
                primary_free_space: row.get(7)?,
                primary_free_inodes: row.get(8)?,
                secondary_free_space: row.get(9)?,
                secondary_free_inodes: row.get(10)?,
            })
        })?
        .try_collect()?;

    Ok(res)
}

pub(crate) fn insert(
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

    tx.execute(
        r#"
        INSERT INTO buddy_groups (node_type) VALUES (?1)
        "#,
        [node_type],
    )?;

    let inserted_uid: BuddyGroupUID = tx.last_insert_rowid().into();

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
        params![new_id, inserted_uid, primary_target_id, secondary_target_id],
    )?;

    Ok(new_id)
}

pub(crate) fn update_storage_pools(
    tx: &mut Transaction,
    new_pool_id: StoragePoolID,
    move_ids: impl IntoIterator<Item = BuddyGroupID>,
) -> Result<()> {
    let mut stmt = tx.prepare_cached(
        r#"
        UPDATE storage_buddy_groups SET pool_id = ?1 WHERE buddy_group_id = ?2
        "#,
    )?;

    for t in move_ids {
        stmt.execute(params![new_pool_id, t])?;
    }

    Ok(())
}

pub(crate) fn check_and_swap_buddies(
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

pub(crate) fn prepare_storage_deletion(
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

pub(crate) fn delete_storage(tx: &mut Transaction, id: BuddyGroupID) -> Result<()> {
    let affected = tx.execute(
        r#"
        DELETE FROM buddy_groups
        WHERE buddy_group_uid = (
            SELECT buddy_group_uid FROM storage_buddy_groups WHERE buddy_group_id = ?1
        )
        "#,
        [id],
    )?;

    ensure_rows_modified!(affected, id);

    Ok(())
}
