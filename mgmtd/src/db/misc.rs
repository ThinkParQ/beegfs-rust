//! Miscellaneous functions for database interaction and other business logic.

use super::*;
use rusqlite::types::FromSql;
use std::ops::RangeInclusive;

/// Finds a new unused ID from a specified table within a given range.
///
/// `table` is the SQL table name and `field` the ID field that should be queried. `range` limits
/// the squery to a given numerical range.
///
/// It tries by the following order:
/// 1. The smallest unused id within the allowed range
/// 2. The minimum value if unused (this happens when the table is empty)
///
/// # Return value
/// Returns an unused and available ID using the given constraints. If there is none available, an
/// error is returned.
///
/// # Warning
/// Vulnerable to sql injection, do not call with user supplied input!
pub(crate) fn find_new_id<T: FromSql + std::fmt::Display>(
    tx: &Transaction,
    table: &str,
    field: &str,
    range: RangeInclusive<T>,
) -> Result<T> {
    let min = range.start();
    let max = range.end();

    let id = tx.query_row(
        &format!(
            "SELECT COALESCE(
                (SELECT MIN(t1.{field}) + 1 AS new
                    FROM {table} AS t1
                    LEFT JOIN {table} AS t2 ON t2.{field} = t1.{field} + 1
                    WHERE t2.{field} IS NULL AND t1.{field} + 1 BETWEEN {min} AND {max}
                ),
                (SELECT {min} WHERE NOT EXISTS
                    (SELECT NULL FROM {table} WHERE {field} = {min})
                )
            )"
        ),
        [],
        |row| row.get::<_, T>(0),
    )?;

    Ok(id)
}

/// Information about the meta root of the BeeGFS installation.
///
/// Contains info on which type of meta root there is and on which node or buddy group it is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MetaRoot {
    Unknown,
    Normal(NodeId, Uid),
    Mirrored(BuddyGroupId),
}

/// Retrieves the meta root information of the BeeGFS system.
pub(crate) fn get_meta_root(tx: &Transaction) -> Result<MetaRoot> {
    let res = tx
        .query_row_cached(
            sql!(
                "SELECT mt.node_id, mn.node_uid, ri.group_id
                FROM root_inode AS ri
                LEFT JOIN meta_targets AS mt ON mt.target_id = ri.target_id
                LEFT JOIN meta_nodes AS mn ON mn.node_id = mt.node_id"
            ),
            [],
            |row| {
                Ok(match row.get::<_, Option<NodeId>>(0)? {
                    Some(node_id) => MetaRoot::Normal(node_id, row.get(1)?),
                    None => MetaRoot::Mirrored(row.get(2)?),
                })
            },
        )
        .optional()?;

    Ok(match res {
        Some(meta_root) => meta_root,
        None => MetaRoot::Unknown,
    })
}

/// Switch the system over to use a buddy mirror group as meta root.
///
/// Gets the meta target with the root inode and moves the root inode to the buddy group which
/// contains that target as primary target. Then a resync for the secondary target is triggered.
pub(crate) fn enable_metadata_mirroring(tx: &Transaction) -> Result<()> {
    let affected = tx.execute(
        sql!(
            "UPDATE root_inode
            SET target_id = NULL, group_id = (
                SELECT mg.group_id FROM root_inode AS ri
                INNER JOIN meta_buddy_groups AS mg ON mg.p_target_id = ri.target_id
            )"
        ),
        [],
    )?;

    check_affected_rows(affected, [1])?;

    let affected = tx.execute(
        sql!(
            "UPDATE targets SET consistency = 'needs_resync'
            WHERE target_uid = (
                SELECT mt.target_uid FROM root_inode AS ri
                INNER JOIN meta_buddy_groups AS mg USING(group_id)
                INNER JOIN meta_targets AS mt ON mt.target_id = mg.s_target_id
            )"
        ),
        [],
    )?;

    check_affected_rows(affected, [1])
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn find_new_id() {
        with_test_data(|tx| {
            // New max id
            let new_id = super::find_new_id(tx, "meta_targets", "target_id", 1..=100).unwrap();
            assert_eq!(new_id, 5);
            // New min ID in a non-empty range
            let new_id = super::find_new_id(tx, "meta_targets", "target_id", 0..=4).unwrap();
            assert_eq!(new_id, 0);
            // New min ID in an empty range
            let new_id = super::find_new_id(tx, "meta_targets", "target_id", 100..=101).unwrap();
            assert_eq!(new_id, 100);

            // All IDs taken
            super::find_new_id(tx, "meta_targets", "target_id", 1..=4).unwrap_err();
        })
    }

    #[test]
    fn meta_root() {
        with_test_data(|tx| {
            let meta_root = super::get_meta_root(tx).unwrap();
            assert_eq!(MetaRoot::Normal(1, 101001i64), meta_root);

            super::enable_metadata_mirroring(tx).unwrap();

            let meta_root = super::get_meta_root(tx).unwrap();
            assert_eq!(MetaRoot::Mirrored(1), meta_root);

            super::enable_metadata_mirroring(tx).unwrap_err();
        })
    }
}
