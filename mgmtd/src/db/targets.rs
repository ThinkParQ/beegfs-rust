use super::*;
use std::time::Duration;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Target {
    pub target_uid: TargetUID,
    pub target_id: TargetID,
    pub node_type: NodeTypeServer,
    pub node_uid: NodeUID,
    pub node_id: NodeID,
    pub pool_id: Option<StoragePoolID>,
    pub free_space: Option<u64>,
    pub free_inodes: Option<u64>,
    pub consistency: TargetConsistencyState,
    pub last_contact: Duration,
}

pub fn with_type(tx: &mut Transaction, node_type: NodeTypeServer) -> Result<Vec<Target>> {
    let mut stmt = tx.prepare_cached(
        r#"
        SELECT target_uid, target_id, node_type, node_uid, node_id, pool_id,
            free_space, free_inodes, consistency, last_contact_s
        FROM all_targets_v
        WHERE node_type = ?1 AND node_id IS NOT NULL;
        "#,
    )?;

    let res = stmt
        .query_map([node_type], |row| {
            Ok(Target {
                target_uid: row.get(0)?,
                target_id: row.get(1)?,
                node_type: row.get(2)?,
                node_uid: row.get(3)?,
                node_id: row.get(4)?,
                pool_id: row.get(5)?,
                free_space: row.get(6)?,
                free_inodes: row.get(7)?,
                consistency: row.get(8)?,
                last_contact: Duration::from_secs(row.get(9)?),
            })
        })?
        .try_collect()?;

    Ok(res)
}

pub fn insert_meta(tx: &mut Transaction, node_id: NodeID, alias: &EntityAlias) -> Result<()> {
    insert(
        tx,
        u16::from(node_id).into(),
        NodeTypeServer::Meta,
        Some(node_id),
        alias,
    )?;

    // If this is the first meta target, set it as meta root
    tx.execute(
        r#"
        INSERT OR IGNORE INTO root_inode (target_id) VALUES (?1)
        "#,
        params![node_id],
    )?;

    Ok(())
}

pub fn insert_or_ignore_storage(
    tx: &mut Transaction,
    target_id: Option<TargetID>,
    alias: &EntityAlias,
) -> Result<TargetID> {
    let target_id = if let Some(target_id) = target_id {
        let mut stmt = tx.prepare_cached(
            r#"
            SELECT COUNT(*) FROM storage_targets_v WHERE target_id = ?1
            "#,
        )?;

        let count = stmt.query_row(params![target_id], |row| row.get::<_, i32>(0))?;

        drop(stmt);

        if count == 0 {
            insert(tx, target_id, NodeTypeServer::Storage, None, alias)?;
        }

        target_id
    } else {
        let target_id = misc::find_new_id(tx, "storage_targets", "target_id", 1..=0xFFFF)?.into();
        insert(tx, target_id, NodeTypeServer::Storage, None, alias)?;
        target_id
    };

    Ok(target_id)
}

fn insert(
    tx: &mut Transaction,
    target_id: TargetID,
    node_type: NodeTypeServer,
    node_id: Option<NodeID>,
    alias: &EntityAlias,
) -> Result<()> {
    println!("AAA insert");
    let mut stmt = tx.prepare_cached(
        r#"
        INSERT INTO entities (entity_type, alias) VALUES ("target", ?1)
        "#,
    )?;

    stmt.execute(params![alias])?;

    let new_uid: TargetUID = tx.last_insert_rowid().into();

    tx.execute(
        r#"
        INSERT INTO targets (target_uid, node_type)
        VALUES (?1, ?2)
        "#,
        params![new_uid, node_type],
    )?;

    tx.execute(
        &format!(
            r#"
            INSERT INTO {node_type}_targets (target_id, target_uid, node_id)
            VALUES (?1, ?2, ?3)
            "#,
        ),
        params![target_id, new_uid, node_id],
    )?;

    Ok(())
}

pub fn update_consistency_states(
    tx: &mut Transaction,
    changes: impl IntoIterator<Item = (TargetID, TargetConsistencyState)>,
    node_type: NodeTypeServer,
) -> Result<usize> {
    let mut update = tx.prepare_cached(
        r#"
        UPDATE targets SET consistency = ?3
        WHERE consistency != ?3 AND target_uid = (
            SELECT target_uid FROM all_targets_v WHERE target_id = ?1 AND node_type = ?2
        )
        "#,
    )?;

    let mut affected = 0;
    for e in changes {
        affected += update.execute(params![e.0, node_type, e.1])?;
    }

    Ok(affected)
}

pub fn update_storage_pools(
    tx: &mut Transaction,
    new_pool_id: StoragePoolID,
    move_ids: impl IntoIterator<Item = TargetID>,
) -> Result<()> {
    let mut stmt = tx.prepare_cached(
        r#"
        UPDATE storage_targets SET pool_id = ?1 WHERE target_id = ?2
        "#,
    )?;

    for t in move_ids {
        stmt.execute(params![new_pool_id, t])?;
    }

    Ok(())
}

pub fn update_storage_node_mapping(
    tx: &mut Transaction,
    target_id: TargetID,
    new_node_id: NodeID,
) -> Result<()> {
    let mut stmt = tx.prepare_cached(
        r#"
        UPDATE storage_targets SET node_id = ?1 WHERE target_id = ?2
        "#,
    )?;

    let affected = stmt.execute(params![new_node_id, target_id])?;

    ensure_rows_modified!(affected, target_id);
    Ok(())
}

pub struct TargetCapacities {
    pub total_space: Option<u64>,
    pub total_inodes: Option<u64>,
    pub free_space: Option<u64>,
    pub free_inodes: Option<u64>,
}

pub fn get_and_update_capacities(
    tx: &mut Transaction,
    items: impl IntoIterator<Item = (TargetID, TargetCapacities)>,
    node_type: NodeTypeServer,
) -> Result<Vec<(TargetID, TargetCapacities)>> {
    let mut select = tx.prepare_cached(
        r#"
        SELECT total_space, total_inodes, free_space, free_inodes
        FROM all_targets_v
        WHERE target_id = ?1 AND node_type = ?2;
        "#,
    )?;

    let mut update = tx.prepare_cached(
        r#"
        UPDATE targets
        SET total_space = ?1, total_inodes = ?2, free_space = ?3, free_inodes = ?4
        WHERE target_uid = (
            SELECT target_uid FROM all_targets_v WHERE target_id = ?5 AND node_type = ?6
        )
        "#,
    )?;

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

pub fn delete_storage(tx: &mut Transaction, target_id: TargetID) -> Result<()> {
    let affected = tx.execute(
        r#"
        DELETE FROM storage_targets WHERE target_id = ?1
        "#,
        params![target_id],
    )?;
    ensure_rows_modified!(affected, target_id);

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::db::test::*;

    #[test]
    fn insert_meta() {
        with_test_data(|tx| {
            super::insert_meta(tx, 1.into(), &"existing_meta_target".into()).unwrap_err();
            super::insert_meta(tx, 99.into(), &"new_meta_target".into()).unwrap();
            // existing alias
            super::insert_meta(tx, 99.into(), &"new_meta_target".into()).unwrap_err();

            let targets = super::with_type(tx, NodeTypeServer::Meta).unwrap();

            assert_eq!(5, targets.len());
        })
    }

    #[test]
    fn insert_storage_and_map() {
        with_test_data(|tx| {
            let new_target_id =
                super::insert_or_ignore_storage(tx, None, &"new_storage_target".into()).unwrap();
            super::insert_or_ignore_storage(tx, Some(1000.into()), &"new_storage_target_2".into())
                .unwrap();

            // existing alias
            super::insert_or_ignore_storage(tx, None, &"new_storage_target".into()).unwrap_err();

            super::update_storage_node_mapping(tx, new_target_id, 1.into()).unwrap();
            super::update_storage_node_mapping(tx, 1000.into(), 1.into()).unwrap();

            super::update_storage_node_mapping(tx, 9999.into(), 1.into()).unwrap_err();
            super::update_storage_node_mapping(tx, 1.into(), 9999.into()).unwrap_err();

            let targets = super::with_type(tx, NodeTypeServer::Storage).unwrap();

            assert_eq!(18, targets.len());

            assert!(targets.iter().any(|e| e.target_id == new_target_id));
            assert!(targets.iter().any(|e| e.target_id == 1000.into()));
        })
    }
}
