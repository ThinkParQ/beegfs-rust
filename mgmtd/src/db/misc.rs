//! Miscellaneous functions for database interaction and other business logic.

use super::*;
use pb::beegfs::beegfs;
use pb::beegfs::beegfs::{entity_id_variant, EntityIdVariant, EntityType};
use rusqlite::types::FromSql;
use std::ops::RangeInclusive;

/// Finds a new unused ID from a specified table within a given range.
///
/// `table` is the SQL table name and `field` the ID field that should be queried. `range` limits
/// the squery to a given numerical range.
///
/// It tries by the following order:
/// 1. The biggest unused id within the allowed range
/// 2. The smallest unused id within the allowed range
/// 3. The minimum value if unused (this happens when the table is empty)
///
/// # Return value
/// Returns an unused and available ID using the given constraints. If there is none available, an
/// error is returned.
///
/// # Warning
/// Vulnerable to sql injection, do not call with user supplied input!
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
            "SELECT COALESCE(
                (SELECT MAX(t1.{field}) + 1 AS new
                    FROM {table} AS t1
                    LEFT JOIN {table} AS t2 ON t2.{field} = t1.{field} + 1
                    WHERE t2.{field} IS NULL AND t1.{field} + 1 BETWEEN {min} AND {max}
                ),
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
    Normal(NodeID, EntityUID),
    Mirrored(BuddyGroupID),
}

/// Retrieves the meta root information of the BeeGFS system.
pub(crate) fn get_meta_root(tx: &mut Transaction) -> Result<MetaRoot> {
    let res = tx
        .query_row_cached(
            sql!(
                "SELECT mt.node_id, mn.node_uid, ri.buddy_group_id
                FROM root_inode AS ri
                LEFT JOIN meta_targets AS mt ON mt.target_id = ri.target_id
                LEFT JOIN meta_nodes AS mn ON mn.node_id = mt.node_id"
            ),
            [],
            |row| {
                Ok(match row.get::<_, Option<NodeID>>(0)? {
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
pub(crate) fn enable_metadata_mirroring(tx: &mut Transaction) -> Result<()> {
    let affected = tx.execute(
        sql!(
            "UPDATE root_inode
            SET target_id = NULL, buddy_group_id = (
                SELECT mg.buddy_group_id FROM root_inode AS ri
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
                INNER JOIN meta_buddy_groups AS mg USING(buddy_group_id)
                INNER JOIN meta_targets AS mt ON mt.target_id = mg.s_target_id
            )"
        ),
        [],
    )?;

    check_affected_rows(affected, [1])
}

pub(crate) fn uid_from_proto_entity_id(
    tx: &mut Transaction,
    entity_id: EntityIdVariant,
) -> Result<EntityUID> {
    let uid = match entity_id.variant.as_ref().unwrap() {
        entity_id_variant::Variant::Uid(ref uid) => {
            let res: Option<EntityUID> = tx
                .query_row_cached(
                    sql!("SELECT uid FROM entities WHERE uid = ?1"),
                    [uid],
                    |row| row.get(0),
                )
                .optional()?;
            res.ok_or_else(|| anyhow!("uid {uid} doesn't exist"))?
        }
        entity_id_variant::Variant::LegacyId(legacy_id) => match legacy_id.entity_type() {
            EntityType::Unspecified => bail!("unable to determine entity type"),
            EntityType::Node => {
                let nt = match legacy_id.node_type() {
                    beegfs::NodeType::Client => NodeType::Client,
                    beegfs::NodeType::Meta => NodeType::Meta,
                    beegfs::NodeType::Storage => NodeType::Storage,
                    beegfs::NodeType::Management => NodeType::Management,
                    t => bail!("invalid node type: {t:?}"),
                };

                node::get_uid(tx, legacy_id.num_id, nt)?.ok_or_else(|| {
                    anyhow!("node {}:{} doesn't exist", nt.sql_str(), legacy_id.num_id)
                })?
            }
            EntityType::Target => {
                let nt = match legacy_id.node_type() {
                    beegfs::NodeType::Meta => NodeTypeServer::Meta,
                    beegfs::NodeType::Storage => NodeTypeServer::Storage,
                    t => bail!("invalid node type: {t:?}"),
                };

                target::get_uid(tx, legacy_id.num_id.try_into()?, nt)?.ok_or_else(|| {
                    anyhow!("target {}:{} doesn't exist", nt.sql_str(), legacy_id.num_id)
                })?
            }
            EntityType::BuddyGroup => {
                let nt = match legacy_id.node_type() {
                    beegfs::NodeType::Meta => NodeTypeServer::Meta,
                    beegfs::NodeType::Storage => NodeTypeServer::Storage,
                    t => bail!("invalid node type: {t:?}"),
                };

                buddy_group::get_uid(tx, legacy_id.num_id.try_into()?, nt)?.ok_or_else(|| {
                    anyhow!(
                        "buddy group {}:{} doesn't exist",
                        nt.sql_str(),
                        legacy_id.num_id
                    )
                })?
            }
            EntityType::StoragePool => storage_pool::get_uid(tx, legacy_id.num_id.try_into()?)?
                .ok_or_else(|| {
                    anyhow!("storage pool storage:{} doesn't exist", legacy_id.num_id)
                })?,
        },
        entity_id_variant::Variant::Alias(ref alias) => {
            entity::get_uid(tx, alias)?.ok_or_else(|| anyhow!("alias {alias} doesn't exist"))?
        }
    };

    Ok(uid)
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
            assert_eq!(MetaRoot::Normal(1, 101001u64), meta_root);

            super::enable_metadata_mirroring(tx).unwrap();

            let meta_root = super::get_meta_root(tx).unwrap();
            assert_eq!(MetaRoot::Mirrored(1), meta_root);

            super::enable_metadata_mirroring(tx).unwrap_err();
        })
    }
}
