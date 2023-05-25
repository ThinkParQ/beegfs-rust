use crate::ensure_rows_modified;
use anyhow::{anyhow, bail, Context, Result};
use rusqlite::config::DbConfig;
use rusqlite::{params, OpenFlags, Transaction};
use shared::*;
use std::fmt::Debug;
use std::path::Path;
use thiserror::Error;

pub mod buddy_groups;
pub mod cap_pools;
pub mod config;
mod handle;
pub mod misc;
pub mod node_nics;
pub mod nodes;
pub mod quota_default_limits;
pub mod quota_entries;
pub mod quota_limits;
pub mod storage_pools;
pub mod targets;

pub use handle::Handle;

#[derive(Debug, Error)]
#[error("{0} doesn't exist")]
pub struct NonexistingKey(pub(crate) String);

pub fn setup_connection(conn: &mut rusqlite::Connection) -> Result<()> {
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY, true)?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER, true)?;
    conn.pragma_update(None, "journal_mode", "DELETE")?;
    conn.pragma_update(None, "synchronous", "ON")?;

    Ok(())
}

pub fn initialize(path: impl AsRef<Path> + Debug) -> Result<()> {
    if std::fs::try_exists(&path)? {
        bail!("Database file {path:?} already exists");
    }

    std::fs::create_dir_all(
        path.as_ref().parent().ok_or_else(|| {
            anyhow!("Could not determine parent folder of database file {path:?}")
        })?,
    )?;

    std::fs::File::create(&path)
        .with_context(|| format!("Creating database file {path:?} failed"))?;

    let mut conn = rusqlite::Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;

    setup_connection(&mut conn)?;

    conn.execute_batch(include_str!("db/schema/schema.sql"))
        .context("Creating database schema failed")?;

    conn.execute_batch(include_str!("db/schema/views.sql"))
        .context("Creating database views failed")?;

    Ok(())
}

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

    use super::*;
    use rusqlite::Connection;
    use std::sync::atomic::{AtomicU64, Ordering};
    pub use test::Bencher;

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
        setup_connection(&mut conn).unwrap();

        conn.execute_batch(include_str!("db/schema/test_data.sql"))
            .unwrap();

        conn
    }

    pub fn transaction(conn: &mut Connection, op: impl FnOnce(&mut Transaction)) {
        let mut tx = conn.transaction().unwrap();
        op(&mut tx);
        tx.commit().unwrap();
    }
}
