use super::*;
use rusqlite::OptionalExtension;
use std::time::Duration;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct Target {
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

pub(crate) fn with_type(tx: &mut Transaction, node_type: NodeTypeServer) -> Result<Vec<Target>> {
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

pub(crate) fn insert_meta(
    tx: &mut Transaction,
    node_id: NodeID,
    alias: &EntityAlias,
) -> Result<()> {
    let mut stmt = tx.prepare_cached(
        r#"
        INSERT INTO entities (entity_type) VALUES ("target")
        "#,
    )?;

    stmt.execute(params![])?;

    let new_uid: TargetUID = tx.last_insert_rowid().into();

    tx.execute(
        r#"
        INSERT INTO targets (target_uid, node_type, alias)
        VALUES (?1, "meta", ?2)
        "#,
        params![new_uid, alias],
    )?;

    tx.execute(
        r#"
        INSERT INTO meta_targets (target_id, target_uid, node_id)
        VALUES (?1, ?2, ?1)
        "#,
        params![node_id, new_uid],
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

pub(crate) fn insert_storage_if_new(
    tx: &mut Transaction,
    target_id: TargetID,
    alias: EntityAlias,
) -> Result<TargetID> {
    fn insert(tx: &mut Transaction, target_id: TargetID, alias: &EntityAlias) -> Result<()> {
        let mut stmt = tx.prepare_cached(
            r#"
            INSERT INTO entities (entity_type) VALUES ("target")
            "#,
        )?;

        stmt.execute(params![])?;

        let new_uid: TargetUID = tx.last_insert_rowid().into();

        tx.execute(
            r#"
            INSERT INTO targets (target_uid, node_type, alias)
            VALUES (?1, "storage", ?2)
            "#,
            params![new_uid, alias,],
        )?;

        let last_uid: TargetUID = tx.last_insert_rowid().into();

        tx.execute(
            r#"
            INSERT INTO storage_targets (target_id, target_uid)
            VALUES (?1, ?2)
            "#,
            params![target_id, last_uid],
        )?;

        Ok(())
    }

    if target_id == TargetID::ZERO {
        if let Some(matching_id) = tx
            .query_row(
                r#"
                SELECT target_id FROM storage_targets_v WHERE alias = ?1
                "#,
                [&alias],
                |row| row.get(0),
            )
            .optional()?
        {
            Ok(matching_id)
        } else {
            let new_id = misc::find_new_id(tx, "storage_targets", "target_id", 1..=0xFFFF)?.into();
            insert(tx, new_id, &alias)?;
            Ok(new_id)
        }
    } else if 1
        == tx.query_row(
            r#"
            SELECT COUNT(*) FROM storage_targets_v WHERE target_id = ?1 AND alias = ?2
            "#,
            params![target_id, alias],
            |row| row.get::<_, i32>(0),
        )?
    {
        // both IDs given and they match with existing entry
        Ok(target_id)
    } else if 1
        == tx.query_row(
            r#"
            SELECT COUNT(*) FROM storage_targets_v WHERE alias = ?1
            "#,
            params![alias],
            |row| row.get::<_, i32>(0),
        )?
    {
        bail!(format!("{alias:?} conflicts with existing entry"));
    } else if 1
        == tx.query_row(
            r#"
            SELECT COUNT(*) FROM storage_targets_v WHERE target_id = ?1
            "#,
            params![target_id],
            |row| row.get::<_, i32>(0),
        )?
    {
        bail!(format!("{target_id:?} conflicts with existing entry"));
    } else {
        // both IDs given and none of them is already used
        insert(tx, target_id, &alias)?;
        Ok(target_id)
    }
}

pub(crate) fn update_consistency_states(
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

pub(crate) fn update_storage_pools(
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

pub(crate) fn update_node(
    tx: &mut Transaction,
    target_id: TargetID,
    node_type: NodeTypeServer,
    new_node_id: NodeID,
) -> Result<()> {
    let mut stmt = tx.prepare_cached(&format!(
        r#"
        UPDATE {}_targets SET node_id = ?1 WHERE target_id = ?2
        "#,
        node_type.as_sql_str(),
    ))?;

    let affected = stmt.execute(params![new_node_id, target_id])?;

    ensure_rows_modified!(affected, target_id);
    Ok(())
}

pub(crate) struct TargetCapacities {
    pub total_space: Option<u64>,
    pub total_inodes: Option<u64>,
    pub free_space: Option<u64>,
    pub free_inodes: Option<u64>,
}

pub(crate) fn get_and_update_capacities(
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

pub(crate) fn delete_storage(tx: &mut Transaction, target_id: TargetID) -> Result<()> {
    let target_uid: TargetUID = tx.query_row(
        r#"
        SELECT target_uid FROM storage_targets_v WHERE target_id = ?1
        "#,
        [target_id],
        |row| row.get(0),
    )?;

    tx.execute(
        r#"
        DELETE FROM targets WHERE target_uid = ?1
        "#,
        params![target_uid],
    )?;

    tx.execute(
        r#"
        DELETE FROM entities WHERE uid = ?1
        "#,
        [target_uid],
    )?;

    Ok(())
}
