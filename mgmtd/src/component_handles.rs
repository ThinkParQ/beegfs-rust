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
    pub db: db::Handle,
    pub static_config: &'static crate::StaticInfo,
    pub config_input: ::config::CacheInput<BeeConfig>,
    pub config: ::config::Cache<BeeConfig>,
}

#[async_trait]
impl ComponentInteractor for ComponentHandles {
    async fn execute_db<
        T: Send + 'static + FnOnce(&mut Transaction) -> Result<R>,
        R: Send + 'static,
    >(
        &self,
        op: T,
    ) -> Result<R> {
        self.db.execute(op).await
    }

    async fn request<M: Msg, R: Msg>(&self, dest: PeerID, msg: &M) -> Result<R, anyhow::Error> {
        MgmtdPool::request(&self.conn, dest, msg).await
    }

    async fn send<M: Msg + BeeSerde>(&self, dest: PeerID, msg: &M) -> Result<(), anyhow::Error> {
        MgmtdPool::send(&self.conn, dest, msg).await
    }

    async fn notify_nodes<M: Notification<'static> + Send>(&self, msg: &M) {
        notify_nodes(&self.conn, &self.db, msg).await
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
