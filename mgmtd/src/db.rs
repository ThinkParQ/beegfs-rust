use crate::ensure_rows_modified;
use anyhow::{bail, Result};
use rusqlite::{params, Transaction};
use shared::*;
#[cfg(test)]
use test::with_test_data;
use thiserror::Error;

pub(crate) mod buddy_groups;
pub(crate) mod cap_pools;
pub(crate) mod config;
pub(crate) mod logic;
pub(crate) mod misc;
pub(crate) mod node_nics;
pub(crate) mod nodes;
pub(crate) mod quota_default_limits;
pub(crate) mod quota_entries;
pub(crate) mod quota_limits;
pub(crate) mod sqlite;
pub(crate) mod storage_pools;
pub(crate) mod targets;

pub(crate) use sqlite::Handle;

#[derive(Debug, Error)]
#[error("{0} doesn't exist")]
pub(crate) struct NonexistingKey(pub(crate) String);

#[macro_export]
/// Tests the amount of affected rows after an UPDATE or DELETE, bail if 0
macro_rules! ensure_rows_modified {
    ($affected:ident, $key:expr) => {
        match $affected {
            0 => {
                anyhow::bail!($crate::db::NonexistingKey(format!("{:?}", $key)));
            }
            _ => {
                // Ok
            }
        }
    };

    ($affected:ident, $($key:expr),+) => {
        match $affected {
            0 => {
                let key_tuple = (
                    $(
                        $key,
                    )+
                );
                ::anyhow::bail!($crate::db::NonexistingKey(format!("{key_tuple:?}")));
            }
            _ => {
                // Ok
            }
        }
    };
}

#[cfg(test)]
mod test {
    extern crate test;

    use super::sqlite::setup_connection;
    use super::*;
    use crate::db;
    use test::Bencher;

    pub fn with_test_data(op: impl FnOnce(&mut Transaction)) {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        setup_connection(&mut conn).unwrap();

        // Setup test data
        conn.execute_batch(include_str!("db/schema/schema.sql"))
            .unwrap();
        conn.execute_batch(include_str!("db/schema/views.sql"))
            .unwrap();
        conn.execute_batch(include_str!("db/schema/test_data.sql"))
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
}
