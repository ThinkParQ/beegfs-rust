//! Connection abstraction and functions to initialize the database

use super::*;
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use rusqlite::config::DbConfig;
use rusqlite::{OpenFlags, Transaction};
use shared::conn::{AddrResolver, PeerID};
use std::fmt::Debug;
use std::net::SocketAddr;
use std::path::Path;

pub fn initialize(path: impl AsRef<Path> + Debug) -> Result<()> {
    if std::fs::try_exists(&path)? {
        bail!("Database file {path:?} already exists");
    }

    std::fs::create_dir_all(path.as_ref().parent().ok_or_else(|| {
        DbError::other(format!(
            "Could not determine parent folder of database file {path:?}"
        ))
    })?)?;

    std::fs::File::create(&path)
        .with_context(|| format!("Creating database file {path:?} failed"))?;

    let mut conn = rusqlite::Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;

    connection::setup_connection(&mut conn)?;

    conn.execute_batch(include_str!("schema/schema.sql"))
        .with_context(|| "Creating database schema failed")?;

    conn.execute_batch(include_str!("schema/views.sql"))
        .with_context(|| "Creating database views failed")?;

    Ok(())
}

#[derive(Clone, Debug)]
pub struct Connection {
    conn: tokio_rusqlite::Connection,
}

impl Connection {
    pub async fn open(path: impl AsRef<Path> + Debug) -> Result<Self> {
        let conn =
            tokio_rusqlite::Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE)
                .await?;

        conn.call(setup_connection).await?;

        log::info!("Opened database at {:?}", path);

        Ok(Self { conn })
    }

    pub async fn op<
        T: Send + 'static + FnOnce(&mut Transaction) -> DbResult<R>,
        R: Send + 'static,
    >(
        &self,
        op: T,
    ) -> DbResult<R> {
        self.conn
            .call(move |conn| {
                let mut tx = conn.transaction()?;
                let res = op(&mut tx)?;
                tx.commit()?;

                Ok(res)
            })
            .await
    }
}

#[async_trait]
impl AddrResolver for Connection {
    async fn lookup(&self, generic_addr: PeerID) -> Result<Vec<SocketAddr>> {
        Ok(match generic_addr {
            PeerID::Addr(addr) => {
                vec![addr]
            }
            PeerID::Node(uid) => self
                .op(move |tx| node_nic::get_with_node_uid(tx, uid))
                .await?
                .into_iter()
                .map(|e| SocketAddr::new(e.addr.into(), e.port.into()))
                .collect(),
        })
    }

    async fn reverse_lookup(&self, addr: SocketAddr) -> PeerID {
        PeerID::Addr(addr)
    }
}

pub fn setup_connection(conn: &mut rusqlite::Connection) -> DbResult<()> {
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY, true)?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER, true)?;
    conn.pragma_update(None, "journal_mode", "DELETE")?;
    conn.pragma_update(None, "synchronous", "ON")?;

    Ok(())
}
