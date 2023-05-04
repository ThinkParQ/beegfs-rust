use super::*;
use rusqlite::types::FromSql;
use rusqlite::OptionalExtension;
use std::ops::RangeInclusive;

/// Finds unused ID for specified table in the given range. It tries by the
/// following order: 1. The biggest unused id within the allowed range
/// 2. The smallest unused id within the allowed range
/// 3. The minimum value if unused (this happens when the table is empty)
///
/// If all of these fail, the range is full and an empty result is returned
///
/// Vulnerable to sql injection, do not call with user supplied input
pub(crate) fn find_new_id<T: FromSql + std::fmt::Display>(
    tx: &mut Transaction,
    table: &str,
    field: &str,
    range: RangeInclusive<T>,
) -> Result<T> {
    let min = range.start();
    let max = range.end();

    let id = tx.query_row(
        &format!(
            r#"
            SELECT COALESCE(
                (SELECT {field} + 1 FROM {table}
                    WHERE ({field} + 1)BETWEEN {min} AND {max}
                    ORDER BY {field} DESC LIMIT 1
                ),
                (SELECT {field} + 1 FROM {table} WHERE NOT EXISTS
                    (SELECT NULL FROM {table} AS i WHERE i.{field} = {field} + 1)
                    AND ({field} + 1) BETWEEN {min} AND {max}
                    ORDER BY {field} ASC LIMIT 1
                ),
                (SELECT {min} WHERE NOT EXISTS
                    (SELECT NULL FROM {table} WHERE {field} = {min})
                )
            );
            "#
        ),
        [],
        |row| row.get::<_, T>(0),
    )?;

    Ok(id)
}

pub(crate) enum MetaRoot {
    Unknown,
    Normal(TargetUID, NodeID, NodeUID),
    Mirrored(BuddyGroupID),
}

pub(crate) fn get_meta_root(tx: &mut Transaction) -> Result<MetaRoot> {
    // try to get meta root from targets
    let mut stmt = tx.prepare_cached(
        r#"
        SELECT mt.target_uid, mt.node_id, mn.node_uid, ri.buddy_group_id
        FROM root_inode AS ri
        LEFT JOIN meta_targets AS mt ON mt.target_id = ri.target_id
        LEFT JOIN meta_nodes AS mn ON mn.node_id = mt.node_id
        "#,
    )?;

    Ok(
        match stmt
            .query_row([], |row| {
                Ok(match row.get::<_, Option<TargetUID>>(0)? {
                    Some(target_uid) => MetaRoot::Normal(target_uid, row.get(1)?, row.get(2)?),
                    None => MetaRoot::Mirrored(row.get(3)?),
                })
            })
            .optional()?
        {
            Some(meta_root) => meta_root,
            None => MetaRoot::Unknown,
        },
    )
}

pub(crate) fn enable_metadata_mirroring(
    tx: &mut Transaction,
    primary_target_uid: TargetUID,
) -> Result<()> {
    let affected = tx.execute(
        r#"
        UPDATE root_inode
        SET target_id = NULL, buddy_group_id = (
            SELECT mg.buddy_group_id
            FROM root_inode AS ri
            INNER JOIN meta_buddy_groups AS mg ON mg.primary_target_id = ri.target_id
        )
        "#,
        [primary_target_uid],
    )?;

    ensure_rows_modified!(affected, ());

    let affected = tx.execute(
        r#"
        UPDATE targets SET consistency = "needs_resync"
        WHERE target_uid = (
            SELECT mt.target_uid
            FROM root_inode AS ri
            INNER JOIN meta_buddy_groups AS mg USING(buddy_group_id)
            INNER JOIN meta_targets AS mt ON mt.target_id = mg.secondary_target_id
        )
        "#,
        [],
    )?;

    ensure_rows_modified!(affected, ());

    Ok(())
}
