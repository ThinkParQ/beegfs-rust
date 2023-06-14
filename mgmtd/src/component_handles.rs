use crate::db::DbResult;
use crate::msg::{dispatch_request, ComponentInteractor};
use crate::notification::{notify_nodes, Notification};
use crate::{db, MgmtdPool};
use ::config::{ConfigError, ConfigMap, Field};
use anyhow::Result;
use async_trait::async_trait;
use rusqlite::Transaction;
use shared::bee_serde::BeeSerde;
use shared::config::BeeConfig;
use shared::conn::msg_dispatch::*;
use shared::conn::PeerID;
use shared::msg::Msg;

// TODO find a better name
#[derive(Clone, Debug)]
pub(crate) struct ComponentHandles {
    pub conn: MgmtdPool,
    pub db: db::Connection,
    pub static_config: &'static crate::StaticInfo,
    pub config_input: ::config::CacheInput<BeeConfig>,
    pub config: ::config::Cache<BeeConfig>,
}

#[async_trait]
impl ComponentInteractor for ComponentHandles {
    async fn execute_db<
        T: Send + 'static + FnOnce(&mut Transaction) -> DbResult<R>,
        R: Send + 'static,
    >(
        &self,
        op: T,
    ) -> DbResult<R> {
        self.db.execute(op).await
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

    fn get_config<K: Field<BelongsTo = BeeConfig>>(&self) -> K::Value {
        self.config.get::<K>()
    }

    async fn set_raw_config(&mut self, entries: ConfigMap) -> Result<(), ConfigError> {
        self.config_input.set_raw(entries).await
    }

    fn get_static_info(&self) -> &'static crate::StaticInfo {
        self.static_config
    }
}

#[async_trait]
impl ::config::Source for ComponentHandles {
    async fn get(&self) -> Result<ConfigMap, ::config::BoxedError> {
        self.db.get().await
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
