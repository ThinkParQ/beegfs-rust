//! Testing suppport for the database layer

extern crate test;

use super::*;
use rusqlite::{Connection, Transaction};
use std::sync::atomic::{AtomicU64, Ordering};
pub use test::Bencher;

pub fn with_test_data(op: impl FnOnce(&mut Transaction)) {
    let mut conn = rusqlite::Connection::open_in_memory().unwrap();
    connection::setup_connection(&mut conn).unwrap();

    // Setup test data
    conn.execute_batch(include_str!("schema/schema.sql"))
        .unwrap();
    conn.execute_batch(include_str!("schema/views.sql"))
        .unwrap();
    conn.execute_batch(include_str!("schema/test_data.sql"))
        .unwrap();

    let mut tx = conn.transaction().unwrap();
    op(&mut tx);
    tx.commit().unwrap();
}

static DB_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn setup_benchmark() -> rusqlite::Connection {
    let benchmark_dir =
        std::env::var("BEEGFS_BENCHMARK_DIR").unwrap_or("/tmp/beegfs_benchmarks".to_string());

    let counter = DB_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = format!("{benchmark_dir}/{counter}.db");

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&path);
    initialize(&path).unwrap();

    let mut conn = rusqlite::Connection::open(&path).unwrap();
    connection::setup_connection(&mut conn).unwrap();

    conn.execute_batch(include_str!("schema/test_data.sql"))
        .unwrap();

    conn
}

pub fn transaction(conn: &mut Connection, op: impl FnOnce(&mut Transaction)) {
    let mut tx = conn.transaction().unwrap();
    op(&mut tx);
    tx.commit().unwrap();
}
