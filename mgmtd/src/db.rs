//! Tools and operations related to mgmtds database backend.
//!
//! Managements core functionality is about keeping a consistent system state and providing
//! interfaces to query and update it. This is done by storing all information inside an SQLite
//! database. This module provides the functionality to access it and operations to interact with
//! it. The latter hide the raw SQL and resemble a primitive ORM, defining data models in terms of
//! Rust and interfaces to obtain the data.

mod import_v7;
mod op;
#[cfg(test)]
mod test;

use anyhow::{anyhow, bail, Context, Result};
pub use import_v7::import_v7;
pub(crate) use op::*;
use rusqlite::config::DbConfig;
use rusqlite::{OpenFlags, Transaction};
use std::fmt::Debug;
use std::path::Path;

/// Wraps an async database connection and provides means to use it
#[derive(Clone, Debug)]
pub(crate) struct Connection {
    conn: tokio_rusqlite::Connection,
}

impl Connection {
    /// Opens a new asynchronous SQLite connection.
    pub(crate) async fn open(path: impl AsRef<Path> + Debug) -> Result<Self> {
        let conn =
            tokio_rusqlite::Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE)
                .await?;

        conn.call(|conn| setup_connection(conn).map_err(|err| err.into()))
            .await?;

        log::info!("Opened database at {:?}", path);

        Ok(Self { conn })
    }

    /// Executes code within the database thread.
    ///
    /// Automatically wraps the provided code in a transaction that is committed on successful
    /// completion or rolled in case of an Error.
    ///
    /// Database access is provided using a single thread, so blocking or heavy computation must be
    /// avoided.
    pub(crate) async fn op<
        T: Send + 'static + FnOnce(&mut Transaction) -> Result<R>,
        R: Send + 'static,
    >(
        &self,
        op: T,
    ) -> Result<R> {
        let res = self
            .conn
            .call(move |conn| {
                let mut tx = conn.transaction()?;
                let res = op(&mut tx).map_err(|err| tokio_rusqlite::Error::Other(err.into()))?;
                tx.commit()?;

                Ok(res)
            })
            .await;

        match res {
            Ok(res) => Ok(res),
            Err(err) => {
                if let tokio_rusqlite::Error::Other(other) = err {
                    return Err(anyhow!(other));
                }

                Err(err.into())
            }
        }
    }
}

/// Sets connection parameters on an SQLite connection.
pub fn setup_connection(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    // We use the carray extension to bind arrays to parameters
    rusqlite::vtab::array::load_module(conn)?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY, true)?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER, true)?;
    conn.pragma_update(None, "journal_mode", "DELETE")?;
    conn.pragma_update(None, "synchronous", "ON")?;

    Ok(())
}

pub fn create_file(db_path: &Path) -> Result<()> {
    if db_path.try_exists()? {
        bail!("Database file {db_path:?} already exists");
    }

    std::fs::create_dir_all(db_path.parent().ok_or_else(|| {
        anyhow!("Could not determine parent folder of database file {db_path:?}")
    })?)?;

    std::fs::File::create(db_path)
        .with_context(|| format!("Creating database file {db_path:?} failed"))?;

    Ok(())
}

pub fn create_schema(tx: &mut Transaction) -> Result<()> {
    tx.execute_batch(include_str!("db/schema/schema.sql"))?;

    Ok(())
}

pub fn open(db_path: &Path) -> Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
    )?;

    setup_connection(&conn)?;

    Ok(conn)
}

pub fn open_in_memory() -> Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open_in_memory()?;
    setup_connection(&conn)?;
    Ok(conn)
}
