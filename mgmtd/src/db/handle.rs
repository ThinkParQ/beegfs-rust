use crate::db;
use ::config::{BoxedError, ConfigMap, Source};
use anyhow::Result;
use async_trait::async_trait;
use rusqlite::{OpenFlags, Transaction};
use shared::config::BeeConfig;
use shared::conn::{AddrResolver, PeerID};
use std::collections::HashMap;
use std::fmt::Debug;
use std::net::SocketAddr;
use std::path::Path;
use tokio_rusqlite::Connection;

#[derive(Clone, Debug)]
pub struct Handle {
    conn: Connection,
}

impl Handle {
    pub async fn open(path: impl AsRef<Path> + Debug) -> Result<Self> {
        let conn =
            tokio_rusqlite::Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE)
                .await?;

        conn.call(db::setup_connection).await?;

        log::info!("Opened database at {:?}", path);

        Ok(Self { conn })
    }

    pub async fn execute<
        T: Send + 'static + FnOnce(&mut Transaction) -> Result<R>,
        R: Send + 'static,
    >(
        &self,
        op: T,
    ) -> Result<R> {
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
impl AddrResolver for Handle {
    async fn lookup(&self, generic_addr: PeerID) -> Result<Vec<SocketAddr>> {
        Ok(match generic_addr {
            PeerID::Addr(addr) => {
                vec![addr]
            }
            PeerID::Node(uid) => self
                .execute(move |tx| db::node_nics::with_node_uid(tx, uid))
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

#[async_trait]
impl Source for Handle {
    async fn get(&self) -> Result<ConfigMap, BoxedError> {
        use ::config::Config;

        let mut entries = self.execute(db::config::get).await?;

        let mut complete_map = HashMap::with_capacity(BeeConfig::ALL_KEYS.len());
        for key in BeeConfig::ALL_KEYS {
            if let Some(value) = entries.get_mut(*key) {
                complete_map.insert(key.to_string(), std::mem::take(value));
            } else {
                complete_map.insert(key.to_string(), BeeConfig::default_value(key)?);
            }
        }

        Ok(complete_map)
    }
}
