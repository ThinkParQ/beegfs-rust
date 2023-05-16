use super::*;
use crate::db;
use test::Bencher;
extern crate test;

pub fn on_memory_db(op: impl FnOnce(&mut Transaction)) {
    let mut db = rusqlite::Connection::open_in_memory().unwrap();

    // Setup test data
    // TODO make sure the same config as for main is used
    db.execute_batch(include_str!("schema/schema.sql")).unwrap();
    db.execute_batch(include_str!("schema/views.sql")).unwrap();
    db.execute_batch(include_str!("tests/test_data.sql"))
        .unwrap();

    let mut tx = db.transaction().unwrap();
    op(&mut tx);
    tx.commit().unwrap();
}

#[bench]
fn bench_get_node(b: &mut Bencher) {
    b.iter(|| {
        on_memory_db(|tx| {
            assert_eq!(
                4,
                db::nodes::with_type(tx, NodeType::Storage).unwrap().len()
            );
        })
    })
}
