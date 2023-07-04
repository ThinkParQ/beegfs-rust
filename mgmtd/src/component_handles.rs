use crate::config::{ConfigCache, DynamicConfig};
use crate::db::DbResult;
use crate::msg::{dispatch_request, ComponentInteractor};
use crate::notification::{notify_nodes, Notification};
use crate::{db, MgmtdPool};
use anyhow::Result;
use async_trait::async_trait;
use rusqlite::Transaction;
use shared::bee_serde::BeeSerde;
use shared::conn::msg_dispatch::*;
use shared::conn::PeerID;
use shared::msg::Msg;
use std::sync::RwLockReadGuard;

// TODO find a better name
#[derive(Clone, Debug)]
pub(crate) struct ComponentHandles {
    pub conn: MgmtdPool,
    pub db: db::Connection,
    pub static_info: &'static crate::StaticInfo,
    pub config_cache: ConfigCache,
}

#[async_trait]
impl ComponentInteractor for ComponentHandles {
    async fn db_op<
        T: Send + 'static + FnOnce(&mut Transaction) -> DbResult<R>,
        R: Send + 'static,
    >(
        &self,
        op: T,
    ) -> DbResult<R> {
        self.db.op(op).await
    }

    async fn request<M: Msg, R: Msg>(&self, dest: PeerID, msg: &M) -> Result<R, anyhow::Error> {
        log::debug!(target: "mgmtd::msg", "REQUEST to {:?}: {:?}", dest, msg);
        let response = MgmtdPool::request(&self.conn, dest, msg).await?;
        log::debug!(target: "mgmtd::msg", "RESPONSE RECEIVED from {:?}: {:?}", dest, msg);
        Ok(response)
    }

    async fn send<M: Msg + BeeSerde>(&self, dest: PeerID, msg: &M) -> Result<(), anyhow::Error> {
        log::debug!(target: "mgmtd::msg", "SEND to {:?}: {:?}", dest, msg);
        MgmtdPool::send(&self.conn, dest, msg).await?;
        Ok(())
    }

    async fn notify_nodes<M: Notification<'static> + Send>(&self, msg: &M) {
        log::debug!(target: "mgmtd::msg", "NOTIFICATION to {:?}: {:?}",
            msg.notification_node_types(), msg);

        notify_nodes(&self.conn, &self.db, msg).await;
    }

    fn get_config(&self) -> RwLockReadGuard<DynamicConfig> {
        self.config_cache.get()
    }

    fn get_static_info(&self) -> &'static crate::StaticInfo {
        self.static_info
    }
}

#[async_trait]
impl DispatchRequest for ComponentHandles {
    async fn dispatch_request(
        &mut self,
        req: impl RequestConnectionController + DeserializeMsg,
    ) -> Result<()> {
        dispatch_request(self.clone(), req).await
    }
}
