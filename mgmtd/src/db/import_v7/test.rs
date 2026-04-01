use crate::db::{MIGRATIONS, initial_entries};
use crate::types::SqliteEnumExt;
use shared::types::{BuddyGroupId, NodeId, NodeType, PoolId, QuotaIdType, QuotaType, TargetId};
use sqlite::{TransactionExt, migrate_schema, open_in_memory};
use sqlite_check::sql;
use std::fs::{create_dir_all, remove_dir_all};
use std::panic::catch_unwind;
use std::path::Path;
use std::process::Command;

const TAR_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/db/import_v7/test_data.tar.gz"
);

/// Tests the v7 import function from a fixed v7 management data folder, created using v7.4
#[cfg(not(target_os = "windows"))]
#[test]
fn import_v7() {
    // Setup
    let pid = std::process::id();
    let tmp_dir = std::env::temp_dir().join(format!(".beegfs_import_v7_test_{pid}"));

    create_dir_all(&tmp_dir).unwrap();
    let res = Command::new("tar")
        .args([
            "-xzf",
            TAR_PATH,
            "-C",
            &tmp_dir.to_string_lossy(),
            "--strip-components",
            "1",
        ])
        .output()
        .unwrap();

    assert!(
        res.status.success(),
        "untaring v7 management archive failed: {}",
        String::from_utf8_lossy(&res.stderr)
    );

    // Run the test, making sure the cleanup below is run even if test fails
    let res = catch_unwind(|| import_v7_inner(&tmp_dir));

    remove_dir_all(&tmp_dir).unwrap();

    res.unwrap();
}

fn import_v7_inner(base_path: &Path) {
    let mut conn = open_in_memory().unwrap();
    let tx = conn.transaction().unwrap();

    migrate_schema(&tx, MIGRATIONS).unwrap();
    initial_entries(&tx, None).unwrap();
    super::import_v7(&tx, base_path).unwrap();

    // Check nodes
    let res: Vec<(NodeType, NodeId)> = tx
        .query_map_collect(
            sql!("SELECT node_type, node_id FROM nodes ORDER BY node_type ASC, node_id ASC"),
            [],
            |row| Ok((NodeType::from_row(row, 0)?, row.get(1)?)),
        )
        .unwrap();

    assert_eq!(
        res,
        &[
            (NodeType::Meta, 1),
            (NodeType::Meta, 2),
            (NodeType::Meta, 3),
            (NodeType::Meta, 4),
            (NodeType::Storage, 1),
            (NodeType::Storage, 2),
            (NodeType::Management, 1)
        ]
    );

    // Check targets
    let res: Vec<(NodeType, TargetId, NodeId, Option<PoolId>)> = tx
        .query_map_collect(
            sql!(
                "SELECT node_type, target_id, node_id, pool_id FROM targets
                ORDER BY node_type ASC, target_id ASC"
            ),
            [],
            |row| {
                Ok((
                    NodeType::from_row(row, 0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                ))
            },
        )
        .unwrap();

    assert_eq!(
        res,
        &[
            (NodeType::Meta, 1.into(), 1, None),
            (NodeType::Meta, 2.into(), 2, None),
            (NodeType::Meta, 3.into(), 3, None),
            (NodeType::Meta, 4.into(), 4, None),
            (NodeType::Storage, 1.into(), 1, Some(1)),
            (NodeType::Storage, 2.into(), 1, Some(2)),
            (NodeType::Storage, 3.into(), 1, Some(2)),
            (NodeType::Storage, 4.into(), 2, Some(1)),
            (NodeType::Storage, 5.into(), 2, Some(1)),
            (NodeType::Storage, 6.into(), 2, Some(2)),
        ]
    );

    // Check buddy groups
    let res: Vec<(NodeType, BuddyGroupId, TargetId, TargetId, Option<PoolId>)> = tx
        .query_map_collect(
            sql!(
                "SELECT node_type, group_id, p_target_id, s_target_id, pool_id
                FROM buddy_groups ORDER BY node_type ASC, group_id ASC"
            ),
            [],
            |row| {
                Ok((
                    NodeType::from_row(row, 0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap();

    assert_eq!(
        res,
        &[
            (NodeType::Meta, 1.into(), 1.into(), 2.into(), None),
            (NodeType::Storage, 1.into(), 1.into(), 4.into(), Some(1)),
            (NodeType::Storage, 2.into(), 3.into(), 6.into(), Some(2)),
        ]
    );

    // Check meta root
    let res: (Option<TargetId>, Option<BuddyGroupId>) = tx
        .query_row(
            sql!("SELECT target_id, group_id FROM root_inode"),
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert_eq!(res, (None, Some(1.into())));

    // Check storage pools
    let res: Vec<(PoolId, String)> = tx
        .query_map_collect(
            sql!("SELECT pool_id, alias FROM pools_ext ORDER BY pool_id ASC"),
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert_eq!(res, &[(1, "Default".to_string()), (2, "pool2".to_string())]);

    // Check quota default limits
    let res: Vec<(QuotaType, QuotaIdType, PoolId, u64)> = tx
        .query_map_collect(
            sql!(
                "SELECT quota_type, id_type, pool_id, value FROM quota_default_limits
                ORDER BY quota_type ASC, id_type ASC, pool_id ASC, value ASC"
            ),
            [],
            |row| {
                Ok((
                    QuotaType::from_row(row, 0)?,
                    QuotaIdType::from_row(row, 1)?,
                    row.get(2)?,
                    row.get(3)?,
                ))
            },
        )
        .unwrap();

    assert_eq!(
        res,
        &[
            (QuotaType::Space, QuotaIdType::User, 1, 1000),
            (QuotaType::Space, QuotaIdType::User, 2, 2000),
            (QuotaType::Space, QuotaIdType::Group, 1, 0),
            (QuotaType::Space, QuotaIdType::Group, 2, 0),
            (QuotaType::Inode, QuotaIdType::User, 1, 100),
            (QuotaType::Inode, QuotaIdType::User, 2, 200),
            (QuotaType::Inode, QuotaIdType::Group, 1, 0),
            (QuotaType::Inode, QuotaIdType::Group, 2, 0),
        ]
    );

    // Check quota limits
    let res: Vec<(QuotaType, QuotaIdType, u64, PoolId, u64)> = tx
        .query_map_collect(
            sql!(
                "SELECT quota_type, id_type, quota_id, pool_id, value FROM quota_limits
                ORDER BY quota_type ASC, id_type ASC, quota_id ASC, pool_id ASC, value ASC"
            ),
            [],
            |row| {
                Ok((
                    QuotaType::from_row(row, 0)?,
                    QuotaIdType::from_row(row, 1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap();

    assert_eq!(
        res,
        &[
            (QuotaType::Space, QuotaIdType::User, 0, 1, 1000),
            (QuotaType::Space, QuotaIdType::User, 0, 2, 2000),
            (QuotaType::Space, QuotaIdType::User, 5000, 1, 5000),
            (QuotaType::Inode, QuotaIdType::User, 0, 1, 100),
            (QuotaType::Inode, QuotaIdType::User, 0, 2, 200),
            (QuotaType::Inode, QuotaIdType::User, 5000, 1, 500),
        ]
    );

    tx.commit().unwrap();
}
