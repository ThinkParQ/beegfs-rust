use super::sqlite::initialize;
use super::*;
use crate::db;
use crate::db::Handle;
use std::net::Ipv4Addr;
use test::Bencher;
extern crate test;

async fn open_db() -> Handle {
    let path = "/tmp/testdb.sql";
    let _ = std::fs::remove_file(path);
    initialize(path).unwrap();
    Handle::open(path).await.unwrap()
}

#[tokio::test]
async fn set_get_node() {
    let db = open_db().await;

    let sn = move |db: Handle, id: NodeID, alias: &'static str, enable_registration: bool| async move {
        db.execute(move |tx| {
            db::nodes::set(
                tx,
                enable_registration,
                id,
                NodeType::Meta,
                alias.into(),
                Port::from(8000),
                vec![],
            )
        })
        .await
    };

    sn(db.clone(), NodeID::ZERO, "1", true).await.unwrap();
    sn(db.clone(), NodeID::from(2), "2", true).await.unwrap();
    sn(db.clone(), NodeID::from(2), "2", false).await.unwrap();
    sn(db.clone(), NodeID::from(2), "3", true).await.unwrap();
    sn(db.clone(), NodeID::ZERO, "4", true).await.unwrap();
    sn(db.clone(), NodeID::from(2), "5", true).await.unwrap();
    sn(db.clone(), NodeID::ZERO, "4", true).await.unwrap_err();
    sn(db.clone(), NodeID::ZERO, "6", false).await.unwrap_err();

    let nodes = db
        .execute(|tx| db::nodes::with_type(tx, NodeType::Meta))
        .await
        .unwrap();

    assert_eq!(nodes.len(), 3);
    assert!(nodes.iter().any(|e| e.id == NodeID::from(2)));
    assert!(nodes.iter().any(|e| e.id == NodeID::from(3)));
}

async fn bench_setup() -> Handle {
    let db = open_db().await;

    db.execute(|tx| {
        db::nodes::set(
            tx,
            true,
            NodeID::from(2),
            NodeType::Storage,
            "alias".into(),
            Port::from(8000),
            vec![],
        )
    })
    .await
    .unwrap();

    db
}

#[bench]
fn bench_get_node(b: &mut Bencher) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();

    let db = rt.block_on(bench_setup());

    b.iter(|| {
        rt.block_on(async {
            assert_eq!(
                1,
                db.execute(|tx| db::nodes::with_type(tx, NodeType::Storage))
                    .await
                    .unwrap()
                    .len()
            );
        })
    })
}

#[bench]
fn bench_set_node(b: &mut Bencher) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();

    let db = rt.block_on(bench_setup());

    b.iter(|| {
        rt.block_on(async {
            db.execute(move |tx| {
                db::nodes::set(
                    tx,
                    true,
                    NodeID::from(2),
                    NodeType::Storage,
                    "alias".into(),
                    Port::from(9000),
                    vec![Nic {
                        addr: Ipv4Addr::LOCALHOST,
                        alias: "interface".into(),
                        nic_type: NicType::Ethernet,
                    }],
                )
            })
            .await
            .unwrap();
        })
    })
}

/// Benchmarks the overhead on SQLite operations caused setup (e.g.
/// tokio_rusqlite, transaction)
#[bench]
fn bench_op_overhead(b: &mut Bencher) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();

    let db = rt.block_on(bench_setup());

    b.iter(|| {
        rt.block_on(async {
            db.execute(|_tx| {
                // do nothing
                Ok(())
            })
            .await
            .unwrap();
        })
    })
}
