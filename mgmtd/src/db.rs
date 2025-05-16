//! Tools and operations related to mgmtds database backend.
//!
//! Managements core functionality is about keeping a consistent system state and providing
//! interfaces to query and update it. This is done by storing all information inside an SQLite
//! database. This module provides the functionality to access it and operations to interact with
//! it. The latter hide the raw SQL and resemble a primitive ORM, defining data models in terms of
//! Rust and interfaces to obtain the data.

pub(crate) mod buddy_group;
pub(crate) mod config;
pub(crate) mod entity;
mod import_v7;
pub(crate) mod misc;
pub(crate) mod node;
pub(crate) mod node_nic;
pub(crate) mod quota_usage;
pub(crate) mod storage_pool;
pub(crate) mod target;

use self::config::Config;
use crate::error::TypedError;
use crate::types::*;
use anyhow::{Result, anyhow, bail};
pub use import_v7::import_v7;
use rusqlite::{OptionalExtension, Row, Transaction, params};
use shared::types::*;
use sqlite::*;
use sqlite_check::sql;
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(test)]
use test::with_test_data;
use uuid::Uuid;

/// Include the generated migration list. First element is the migration number, second is the
/// SQL text to execute. The elements are guaranteed to be contiguous, but may start later than 1.
pub const MIGRATIONS: &[sqlite::Migration] = include!(concat!(env!("OUT_DIR"), "/migrations.rs"));

/// Inserts initial entries into a new database. Remember to commit the transaction after calling
/// this function.
///
/// If `fs_uuid` is provided, it will be used. Otherwise, a new FsUUID will be generated.
pub fn initial_entries(tx: &Transaction, fs_uuid: Option<String>) -> Result<()> {
    let uuid = match fs_uuid {
        Some(ref s) => {
            let parsed =
                Uuid::parse_str(s).map_err(|_| anyhow!("Provided fs_uuid is not a valid UUID"))?;
            if parsed.get_version_num() != 4 {
                bail!("Provided fs_uuid is not a valid v4 UUID");
            }
            s.clone()
        }
        None => Uuid::new_v4().to_string(),
    };
    config::set(tx, Config::FsUuid, uuid)?;
    config::set(
        tx,
        Config::FsInitDateSecs,
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
    )?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use rusqlite::{Connection, Transaction};

    /// Sets ups a fresh database instance in memory and fills, with the test data set and provides
    /// a transaction handle.
    pub(crate) fn with_test_data(op: impl FnOnce(&Transaction)) {
        let mut conn = sqlite::open_in_memory().unwrap();

        let tx = conn.transaction().unwrap();
        sqlite::migrate_schema(&tx, MIGRATIONS).unwrap();
        // Setup test data
        tx.execute_batch(include_str!("db/schema/test_data.sql"))
            .unwrap();
        tx.commit().unwrap();

        transaction(&mut conn, op)
    }

    /// Sets up a transaction for the given [rusqlite::Connection] and executes the provided code.
    ///
    /// Meant for tests and does not return results.
    pub(crate) fn transaction(conn: &mut Connection, op: impl FnOnce(&Transaction)) {
        let mut tx = conn.transaction().unwrap();
        op(&mut tx);
        tx.commit().unwrap();
    }
}
