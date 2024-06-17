use anyhow::{anyhow, Result};
use rusqlite::config::DbConfig;
use rusqlite::{Connection, OpenFlags, Transaction};
use std::path::Path;

/// Sets connection parameters on an SQLite connection.
pub fn setup_connection(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    // We use the carray extension to bind arrays to parameters
    rusqlite::vtab::array::load_module(conn)?;

    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY, true)?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER, true)?;

    Ok(())
}

/// Opens an existing sqlite database for read and write, applying the common config options
pub fn open(db_file: impl AsRef<Path>) -> Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open_with_flags(
        db_file,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
    )?;
    setup_connection(&conn)?;
    Ok(conn)
}

/// Opens an existing sqlite database for async read and write, applying the common config options
pub async fn open_async(db_file: impl AsRef<Path>) -> Result<tokio_rusqlite::Connection> {
    let conn =
        tokio_rusqlite::Connection::open_with_flags(&db_file, OpenFlags::SQLITE_OPEN_READ_WRITE)
            .await?;
    conn.call(|conn| setup_connection(conn).map_err(|err| err.into()))
        .await?;
    Ok(conn)
}

/// Opens an in-memory sqlite database, applying the common config
pub fn open_in_memory() -> Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open_in_memory()?;
    setup_connection(&conn)?;
    Ok(conn)
}

/// Adds useful methods to an async rusqlite connection
pub trait ConnectionExt {
    /// Automatically wraps the provided closure in a transaction that is committed on successful
    /// completion or rolled back in case of an Error. Accepts anyhow::Result as Result and handles
    /// conversion between it and internally expected tokio_rusqlite::Result.
    ///
    /// Database access is provided using a single thread, so blocking or heavy computation must be
    /// avoided inside.
    fn op<T: Send + 'static + FnOnce(&Transaction) -> Result<R>, R: Send + 'static>(
        &self,
        op: T,
    ) -> impl std::future::Future<Output = Result<R>> + Send;

    /// Takes the provided closure and executes db operations on it.
    /// Accepts anyhow::Result as Result and handles conversion between it and internally expected
    /// tokio_rusqlite::Result.
    ///
    /// Same as `op()`, but without the automatic transaction. Should only be used if manual
    /// transaction control is required, e.g. committing or rolling back based on external input.
    ///
    /// Database access is provided using a single thread, so blocking or heavy computation must be
    /// avoided inside.
    fn op_with_conn<T: Send + 'static + FnOnce(&mut Connection) -> Result<R>, R: Send + 'static>(
        &self,
        op: T,
    ) -> impl std::future::Future<Output = Result<R>> + Send;
}

impl ConnectionExt for tokio_rusqlite::Connection {
    async fn op<T: Send + 'static + FnOnce(&Transaction) -> Result<R>, R: Send + 'static>(
        &self,
        op: T,
    ) -> Result<R> {
        let res = self
            .call(move |conn| {
                let tx = conn.transaction()?;
                let res = op(&tx).map_err(|err| tokio_rusqlite::Error::Other(err.into()))?;
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

    async fn op_with_conn<
        T: Send + 'static + FnOnce(&mut Connection) -> Result<R>,
        R: Send + 'static,
    >(
        &self,
        op: T,
    ) -> Result<R> {
        let res = self
            .call(move |conn| {
                let res = op(conn).map_err(|err| tokio_rusqlite::Error::Other(err.into()))?;
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
