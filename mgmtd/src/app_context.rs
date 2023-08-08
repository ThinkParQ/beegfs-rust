//! Interfaces and implementations for in-app interaction between tasks or threads.

use crate::config::{ConfigCache, DynamicConfig};
use crate::db;
use crate::db::DbResult;
use crate::msg::dispatch_request;
use anyhow::Result;
use async_trait::async_trait;
use rusqlite::Transaction;
use shared::bee_serde::BeeSerde;
use shared::conn::msg_dispatch::*;
use shared::conn::Pool;
use shared::msg::Msg;
use shared::{log_error_chain, NodeType, NodeUID};
use std::net::SocketAddr;
use std::ops::Deref;
use std::sync::{Arc, RwLockReadGuard};

/// Common interface for interaction with the apps components.
///
/// Parts of the app that need to interact with other parts, e.g. a message handler accessing the
/// database, should accept an implementation of this and use the provided methods for doing so.
#[async_trait]
pub(crate) trait AppContext: Clone + Send + Sync + 'static {
    /// Accesses the database - executes one or more SQL operations.
    ///
    /// # Important
    /// These run in a single thread, so blocking or heavy computation must be avoided. Also,
    /// for cleanliness and testability, raw SQL should not be put directly in here. Instead, the
    /// operations defined in `db::ops` can be used (and extended if needed).
    async fn db_op<T: Send + 'static + FnOnce(&mut Transaction) -> DbResult<R>, R: Send + 'static>(
        &self,
        op: T,
    ) -> DbResult<R>;

    /// Sends a BeeGFS message via stream and expects a response
    async fn request<M: Msg, R: Msg>(&self, dest: NodeUID, msg: &M) -> Result<R, anyhow::Error>;
    /// Sends a BeeGFS message via stream and doesn't expect a response
    async fn send<M: Msg>(&self, dest: NodeUID, msg: &M) -> Result<(), anyhow::Error>;
    /// Notifies a collection of nodes via datagram and doesn't expect a response
    async fn notify_nodes(&self, node_types: &'static [NodeType], msg: &impl Msg);

    /// Obtains read access to the dynamic config struct
    fn get_config(&self) -> RwLockReadGuard<DynamicConfig>;
    /// Obtains read access to [crate::StaticInfo]
    fn get_static_info(&self) -> &'static crate::StaticInfo;

    fn replace_node_addrs(&self, node_uid: NodeUID, addrs: impl Into<Arc<[SocketAddr]>>);
}

/// A collection of Handles used for interacting and accessing the different components of the app.
///
/// This is the actual runtime object that can be shared between tasks. Interfaces should, however,
/// accept any implementation of the AppContext trait instead.
#[derive(Clone, Debug)]
pub(crate) struct AppHandles {
    /// Stores the actual values.
    ///
    /// Wrapped in an Arc since AppHandles is meant to be shared between threads.
    inner: Arc<InnerAppHandles>,
}

impl AppHandles {
    /// Creates a new AppHandles object.
    ///
    /// Takes all the stored handles.
    pub(crate) fn new(
        conn_pool: Pool,
        db: db::Connection,
        config_cache: ConfigCache,
        static_info: &'static crate::StaticInfo,
    ) -> Self {
        Self {
            inner: Arc::new(InnerAppHandles {
                conn_pool,
                db,
                static_info,
                config_cache,
            }),
        }
    }
}

/// Derefs to InnerAppHandle which stores all the handles.
///
/// Allows transparent access.
impl Deref for AppHandles {
    type Target = InnerAppHandles;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// Stores the actual handles.
#[derive(Debug)]
pub(crate) struct InnerAppHandles {
    conn_pool: Pool,
    db: db::Connection,
    static_info: &'static crate::StaticInfo,
    config_cache: ConfigCache,
}

#[async_trait]
impl AppContext for AppHandles {
    async fn db_op<
        T: Send + 'static + FnOnce(&mut Transaction) -> DbResult<R>,
        R: Send + 'static,
    >(
        &self,
        op: T,
    ) -> DbResult<R> {
        self.db.op(op).await
    }

    async fn request<M: Msg, R: Msg>(&self, dest: NodeUID, msg: &M) -> Result<R, anyhow::Error> {
        log::debug!(target: "mgmtd::msg", "REQUEST to {:?}: {:?}", dest, msg);
        let response = Pool::request(&self.conn_pool, dest, msg).await?;
        log::debug!(target: "mgmtd::msg", "RESPONSE RECEIVED from {:?}: {:?}", dest, msg);
        Ok(response)
    }

    async fn send<M: Msg + BeeSerde>(&self, dest: NodeUID, msg: &M) -> Result<(), anyhow::Error> {
        log::debug!(target: "mgmtd::msg", "SEND to {:?}: {:?}", dest, msg);
        Pool::send(&self.conn_pool, dest, msg).await?;
        Ok(())
    }

    async fn notify_nodes(&self, node_types: &'static [NodeType], msg: &impl Msg) {
        log::debug!(target: "mgmtd::msg", "NOTIFICATION to {:?}: {:?}",
            node_types, msg);

        if let Err(err) = async {
            for t in node_types {
                let nodes = self
                    .db
                    .op(move |tx| db::node::get_with_type(tx, *t))
                    .await?;

                self.conn_pool
                    .broadcast_datagram(nodes.into_iter().map(|e| e.uid), msg)
                    .await?;
            }

            Ok(()) as Result<_>
        }
        .await
        {
            log_error_chain!(
                err,
                "Notification msg could not be send to all nodes: {msg:?}"
            )
        }
    }

    fn get_config(&self) -> RwLockReadGuard<DynamicConfig> {
        self.config_cache.get()
    }

    fn get_static_info(&self) -> &'static crate::StaticInfo {
        self.static_info
    }

    fn replace_node_addrs(&self, node_uid: NodeUID, addrs: impl Into<Arc<[SocketAddr]>>) {
        self.conn_pool.replace_node_addrs(node_uid, addrs)
    }
}

#[async_trait]
impl DispatchRequest for AppHandles {
    async fn dispatch_request(&self, req: impl Request) -> Result<()> {
        dispatch_request(self, req).await
    }
}
