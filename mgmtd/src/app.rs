//! Interfaces and implementations for in-app interaction between tasks or threads.

mod runtime;
#[cfg(test)]
pub(crate) mod test;

use crate::StaticInfo;
use anyhow::Result;
use protobuf::license::GetCertDataResult;
pub(crate) use runtime::App;
use rusqlite::{Connection, Transaction};
use shared::bee_msg::Msg;
use shared::bee_serde::{Deserializable, Serializable};
use shared::types::{NodeId, NodeType, Uid};
use std::fmt::Debug;
use std::future::Future;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

pub(crate) trait AppInfo {
    fn static_info(&self) -> &StaticInfo;
}

pub(crate) trait AppDb {
    fn read_tx<T: Send + 'static + FnOnce(&Transaction) -> Result<R>, R: Send + 'static>(
        &self,
        op: T,
    ) -> impl Future<Output = Result<R>> + Send;
    fn write_tx<T: Send + 'static + FnOnce(&Transaction) -> Result<R>, R: Send + 'static>(
        &self,
        op: T,
    ) -> impl Future<Output = Result<R>> + Send;
    fn write_tx_no_sync<T: Send + 'static + FnOnce(&Transaction) -> Result<R>, R: Send + 'static>(
        &self,
        op: T,
    ) -> impl Future<Output = Result<R>> + Send;
    fn conn<T: Send + 'static + FnOnce(&mut Connection) -> Result<R>, R: Send + 'static>(
        &self,
        op: T,
    ) -> impl Future<Output = Result<R>> + Send;
}

pub(crate) trait AppConn {
    /// Sends a [Msg] to a node and receives the response.
    fn request<M: Msg + Serializable, R: Msg + Deserializable>(
        &self,
        node_uid: Uid,
        msg: &M,
    ) -> impl Future<Output = Result<R>> + Send;
    fn send_notifications<M: Msg + Serializable>(
        &self,
        node_types: &'static [NodeType],
        msg: &M,
    ) -> impl Future<Output = ()> + Send;
    fn replace_node_addrs(&self, node_uid: Uid, new_addrs: impl Into<Arc<[SocketAddr]>>);
}

pub(crate) trait AppRunState {
    fn pre_shutdown(&self) -> bool;
    fn notify_client_pulled_state(&self, node_type: NodeType, node_id: NodeId);
}

pub(crate) trait AppLicense {
    fn load_and_verify_cert(&self, cert_path: &Path)
    -> impl Future<Output = Result<String>> + Send;
    fn get_cert_data(&self) -> Result<GetCertDataResult>;
    fn get_num_machines(&self) -> Result<u32>;
}

pub(crate) trait AppAll:
    AppDb + AppConn + AppRunState + AppLicense + AppInfo + Debug + Clone + Send + Sync + 'static
{
}
impl<T> AppAll for T where
    T: AppDb + AppConn + AppRunState + AppLicense + AppInfo + Debug + Clone + Send + Sync + 'static
{
}
