//! Testing support for the database layer

use super::*;
use rusqlite::{Connection, Transaction};

/// Sets ups a fresh database instance in memory and fills, with the test data set and provides a
/// transaction handle.
pub(crate) fn with_test_data(op: impl FnOnce(&mut Transaction)) {
    let mut conn = open_in_memory().unwrap();
    migrate_schema(&mut conn).unwrap();

    // Setup test data
    conn.execute_batch(include_str!("schema/test_data.sql"))
        .unwrap();

    transaction(&mut conn, op)
}

// static DB_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Sets up a fresh database instance on disk and provides the connection handle.
///
/// Does NOT wrap the provided closure in a transaction. To do that, use [transaction()] where
/// appropriate.
///
/// Each test or benchmark using this gets its own database file when run within the same test
/// binary. This allows tests to be run in parallel even if they use an on-disk file. This should of
/// course still be avoided in case of benchmarks (and is by default, when using the experimental
/// Bencher from [test]).
///
/// The disk location of the database instance can be set by the environment variable
/// `BEEGFS_TEST_DB_DIR`.
// pub fn setup_on_disk_db() -> rusqlite::Connection {
//     let benchmark_dir =
//         std::env::var("BEEGFS_TEST_DB_DIR").unwrap_or("/tmp/beegfs_test_db".to_string());
//
//     let counter = DB_COUNTER.fetch_add(1, Ordering::Relaxed);
//     let path = format!("{benchmark_dir}/{counter}.db");
//
//     let _ = std::fs::remove_file(&path);
//     let _ = std::fs::remove_file(&path);
//     initialize(&path).unwrap();
//
//     let conn = rusqlite::Connection::open(&path).unwrap();
//     connection::setup_connection(&conn).unwrap();
//
//     conn.execute_batch(include_str!("schema/test_data.sql"))
//         .unwrap();
//
//     conn
// }

/// Sets up a transaction for the given [rusqlite::Connection] and executes the provided code.
///
/// Meant for tests and does not return results.
pub(crate) fn transaction(conn: &mut Connection, op: impl FnOnce(&mut Transaction)) {
    let mut tx = conn.transaction().unwrap();
    op(&mut tx);
    tx.commit().unwrap();
}

#[test]
fn migration() {
    let mut conn = open_in_memory().unwrap();

    let mut migrations = vec![Migration {
        version: 1,
        sql: "CREATE TABLE t1 (id INTEGER)",
    }];
    migrate_schema_with(&mut conn, &migrations).unwrap();

    migrations.push(Migration {
        version: 2,
        sql: "CREATE TABLE t2 (id INTEGER)",
    });
    migrate_schema_with(&mut conn, &migrations).unwrap();

    migrations.push(Migration {
        version: 3,
        sql: "CREATE TABLE t3 (id INTEGER)",
    });
    migrations.push(Migration {
        version: 4,
        sql: "CREATE TABLE t4 (id INTEGER)",
    });
    migrate_schema_with(&mut conn, &migrations).unwrap();

    let mut migrations = migrations.split_off(3);
    migrations.push(Migration {
        version: 5,
        sql: "DROP TABLE t1",
    });
    migrate_schema_with(&mut conn, &migrations).unwrap();

    let version = conn
        .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
        .unwrap();

    assert_eq!(5, version);

    let tables = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type == 'table' AND name LIKE 't%'",
            [],
            |row| row.get::<_, u32>(0),
        )
        .unwrap();

    assert_eq!(3, tables);

    // Failure on up-to-date db
    migrate_schema_with(&mut conn, &migrations).unwrap_err();

    // Failure on non-contiguous migration sequence
    migrations.push(Migration {
        version: 7,
        sql: "CREATE TABLE t7 (id INTEGER)",
    });
    migrations.push(Migration {
        version: 6,
        sql: "CREATE TABLE t6 (id INTEGER)",
    });
    migrate_schema_with(&mut conn, &migrations).unwrap_err();
}
