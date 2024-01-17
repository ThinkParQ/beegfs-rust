//! Connection abstraction and functions to initialize the database

use super::*;
use anyhow::{anyhow, bail, Context, Result};
use rusqlite::config::DbConfig;
use rusqlite::{OpenFlags, Transaction};
use std::fmt::Debug;
use std::path::Path;

/// Creates a new database and initializes it.
///
/// Automatically creates the parent folder if it doesn't exist yest.
pub fn initialize(path: impl AsRef<Path> + Debug) -> Result<()> {
    if path.as_ref().try_exists()? {
        bail!("Database file {path:?} already exists");
    }

    std::fs::create_dir_all(
        path.as_ref().parent().ok_or_else(|| {
            anyhow!("Could not determine parent folder of database file {path:?}")
        })?,
    )?;

    std::fs::File::create(&path)
        .with_context(|| format!("Creating database file {path:?} failed"))?;

    let conn = rusqlite::Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;

    connection::setup_connection(&conn)?;

    conn.execute_batch(include_str!("schema/schema.sql"))
        .with_context(|| "Creating database schema failed")?;

    Ok(())
}

/// Wraps an async database connection and provides means to use it
#[derive(Clone, Debug)]
pub struct Connection {
    conn: tokio_rusqlite::Connection,
}

impl Connection {
    /// Opens a new asynchronous SQLite connection.
    pub async fn open(path: impl AsRef<Path> + Debug) -> Result<Self> {
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
    /// Automatically wraps the provided code in a transaction that is commited on successful
    /// completion or rolled in case of an Error.
    ///
    /// Database access is provided using a single thread, so blocking or heavy computation must be
    /// avoided.
    pub async fn op<
        T: Send + 'static + FnOnce(&mut Transaction) -> Result<R>,
        R: Send + 'static,
    >(
        &self,
        op: T,
    ) -> Result<R> {
        self.conn
            .call(move |conn| {
                let mut tx = conn.transaction()?;
                let res = op(&mut tx).map_err(|err| tokio_rusqlite::Error::Other(err.into()))?;
                tx.commit()?;

                Ok(res)
            })
            .await
            .map_err(|err| err.into())
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
