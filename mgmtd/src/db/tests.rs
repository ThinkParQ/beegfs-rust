#![cfg(test)]

extern crate test;

use super::sqlite::setup_connection;
use super::*;
use crate::db;
use test::Bencher;

pub fn with_test_data(op: impl FnOnce(&mut Transaction)) {
    let mut conn = rusqlite::Connection::open_in_memory().unwrap();
    setup_connection(&mut conn).unwrap();

    // Setup test data
    conn.execute_batch(include_str!("schema/schema.sql"))
        .unwrap();
    conn.execute_batch(include_str!("schema/views.sql"))
        .unwrap();
    conn.execute_batch(include_str!("tests/test_data.sql"))
        .unwrap();

    let mut tx = conn.transaction().unwrap();
    op(&mut tx);
    tx.commit().unwrap();
}

#[bench]
fn bench_get_node(b: &mut Bencher) {
    b.iter(|| {
        with_test_data(|tx| {
            assert_eq!(
                4,
                db::nodes::with_type(tx, NodeType::Storage).unwrap().len()
            );
        })
    })
}
