use anyhow::Result;
use rusqlite::config::DbConfig;
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Sets connection parameters on an SQLite connection.
pub fn setup_connection(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    // We use the carray extension to bind arrays to parameters
    rusqlite::vtab::array::load_module(conn)?;

    // We want foreign keys and triggers enabled
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY, true)?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER, true)?;

    // Maximum waiting time on immediate transactions if the write lock is already taken.
    // Note that this does NOT apply to upgrading a deferred transaction from read to write,
    // these will fail immediately.
    conn.busy_timeout(Duration::from_secs(30))?;

    // We want to use WAL mode (https://www.sqlite.org/wal.html) as we write a lot and in this
    // mode, a writer does not block readers (they will just see the old state if they started a
    // transaction before the write happened) and writing also should be faster.
    // Note that the WAL is merged into the main db file automatically by SQLite after it has
    // reached a certain size and on the last connection being closed. This could be configured or
    // even disabled so we can run it manually.
    conn.pragma_update(None, "journal_mode", "wal")?;
    // Default to fsync after each transaction. If this is changed, make sure to update the
    // transaction methods below to match the change, especially in run_op() where a temporary
    // change happens.
    conn.pragma_update(None, "synchronous", "full")?;

    Ok(())
}

/// Opens an existing sqlite database for read and write and configures the connection
pub fn open(db_file: impl AsRef<Path>) -> Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open_with_flags(
        db_file,
        // We don't want to accidentally create a nonexisting file, thus we pass this flag
        // explicitly instead of just using open()
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
    )?;
    setup_connection(&conn)?;
    Ok(conn)
}

/// Opens an in-memory sqlite database and configures the connection
pub fn open_in_memory() -> Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open_in_memory()?;
    setup_connection(&conn)?;
    Ok(conn)
}

/// Provides access to the database
#[derive(Debug, Clone)]
pub struct Connections {
    inner: Arc<InnerConnections>,
}

impl Deref for Connections {
    type Target = InnerConnections;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SyncMode {
    Full,
    Normal,
}

#[derive(Debug)]
pub struct InnerConnections {
    conns: Mutex<Vec<Connection>>,
    db_file: PathBuf,
}

impl Connections {
    pub fn new(db_file: impl AsRef<Path>) -> Self {
        Self {
            inner: Arc::new(InnerConnections {
                conns: Mutex::new(vec![]),
                db_file: db_file.as_ref().to_path_buf(),
            }),
        }
    }

    /// Start a new write (immediate) transaction. If doing writes, it is important to use this
    /// instead of `.read()` because here the busy timeout / busy handler actually works as it is
    /// applied before the transaction starts.
    pub async fn write_tx<
        T: Send + 'static + FnOnce(&Transaction) -> Result<R>,
        R: Send + 'static,
    >(
        &self,
        op: T,
    ) -> Result<R> {
        self.run_op(SyncMode::Full, move |conn| {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let res = op(&tx)?;
            tx.commit()?;

            Ok(res)
        })
        .await
    }

    /// Same as `write_tx()`, but changes sqlite sync mode temporarily from `full` to `normal` to
    /// avoid syncing the transaction to disk immediately. Meant for transactions that can cause
    /// heavy load on bigger systems and are not that critical if they get lost.
    pub async fn write_tx_no_sync<
        T: Send + 'static + FnOnce(&Transaction) -> Result<R>,
        R: Send + 'static,
    >(
        &self,
        op: T,
    ) -> Result<R> {
        self.run_op(SyncMode::Normal, move |conn| {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let res = op(&tx)?;
            tx.commit()?;

            Ok(res)
        })
        .await
    }

    /// Start a new read (deferred) transaction. Note that this does not deny writes, instead tries
    /// to upgrade the transaction lazily. If that fails because there is another write going on,
    /// the whole transaction is spoiled and needs to be rolled back
    /// (that's at least what SQLite recommends: https://sqlite.org/lang_transaction.html).
    /// The busy handler / timeout does not apply here.
    pub async fn read_tx<
        T: Send + 'static + FnOnce(&Transaction) -> Result<R>,
        R: Send + 'static,
    >(
        &self,
        op: T,
    ) -> Result<R> {
        self.run_op(SyncMode::Full, move |conn| {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
            let res = op(&tx)?;
            tx.commit()?;

            Ok(res)
        })
        .await
    }

    /// Execute code using a connection handle. This requires the caller to start a transaction
    /// manually if required. Can be used to do custom rollbacks (e.g. for implementing dry runs).
    /// When using this be aware of the different transaction modes (deferred and immediate) and
    /// their consequences with read and write operations.
    pub async fn conn<
        T: Send + 'static + FnOnce(&mut Connection) -> Result<R>,
        R: Send + 'static,
    >(
        &self,
        op: T,
    ) -> Result<R> {
        self.run_op(SyncMode::Full, op).await
    }

    async fn run_op<T: Send + 'static + FnOnce(&mut Connection) -> Result<R>, R: Send + 'static>(
        &self,
        sync_mode: SyncMode,
        op: T,
    ) -> Result<R> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            // Pop a connection from the stack
            let conn = this.conns.lock().unwrap().pop();

            // If there wasn't one left, open a new one.
            // There is currently no explicit limit set to the number of parallel opens.
            // There is an implicit limit though defined by the max number of parallel blocking
            // threads spawned by tokio which can be set by configuring `max_blocking_threads` on
            // the runtime.
            let mut conn = if let Some(conn) = conn {
                conn
            } else {
                open(this.db_file.as_path())?
            };

            match sync_mode {
                SyncMode::Full => {
                    let res = op(&mut conn);
                    // Push the connection to the stack
                    // We assume that sqlite connections never invalidate on errors, so there is no
                    // need to drop them. There might be severe cases where
                    // connections don't work anymore (e.g. one removing or
                    // corrupting the database file, the file system breaks, ...), but these
                    // are unrecoverable anyway and new connections won't fix anything there.
                    this.conns.lock().unwrap().push(conn);

                    res
                }
                SyncMode::Normal => {
                    conn.pragma_update(None, "synchronous", "normal")?;
                    let res = op(&mut conn);
                    // If the sync mode could not be reset (should most likely never happen), we
                    // don't error out as the transaction already completed.
                    // Instead we just dorop it to prevent future usage with FULL mode.
                    if conn.pragma_update(None, "synchronous", "full").is_ok() {
                        this.conns.lock().unwrap().push(conn);
                    } else {
                        log::error!("Failed to change db connection sync mode back to full");
                    }

                    res
                }
            }
        })
        .await?
    }
}
