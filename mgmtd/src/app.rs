//! Interfaces and implementations for in-app interaction between tasks or threads.

mod runtime;
#[cfg(test)]
pub(crate) mod test;

use crate::StaticInfo;
use crate::license::LicensedFeature;
use anyhow::Result;
use protobuf::license::GetCertDataResult;
pub(crate) use runtime::RuntimeApp;
use rusqlite::{Connection, Transaction};
use shared::bee_msg::Msg;
use shared::bee_serde::{Deserializable, Serializable};
use shared::types::{NodeId, NodeType, Uid};
use std::fmt::Debug;
use std::future::Future;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

pub(crate) trait App: Debug + Clone + Send + 'static {
    /// Return a borrow to the applications static, immutable config and derived info
    fn static_info(&self) -> &StaticInfo;

    // Database access

    /// DB Read transaction
    fn db_read_tx<T: Send + 'static + FnOnce(&Transaction) -> Result<R>, R: Send + 'static>(
        &self,
        op: T,
    ) -> impl Future<Output = Result<R>> + Send;

    /// DB write transaction
    fn db_write_tx<T: Send + 'static + FnOnce(&Transaction) -> Result<R>, R: Send + 'static>(
        &self,
        op: T,
    ) -> impl Future<Output = Result<R>> + Send;

    /// DB write transaction without fsync
    fn db_write_tx_no_sync<
        T: Send + 'static + FnOnce(&Transaction) -> Result<R>,
        R: Send + 'static,
    >(
        &self,
        op: T,
    ) -> impl Future<Output = Result<R>> + Send;

    /// Provides access to a DB connection handle, no transaction
    fn db_conn<T: Send + 'static + FnOnce(&mut Connection) -> Result<R>, R: Send + 'static>(
        &self,
        op: T,
    ) -> impl Future<Output = Result<R>> + Send;

    // BeeMsg communication

    /// Send a [Msg] to a node via TCP and receive the response
    fn beemsg_request<M: Msg + Serializable, R: Msg + Deserializable>(
        &self,
        node_uid: Uid,
        msg: &M,
    ) -> impl Future<Output = Result<R>> + Send;

    /// Send a [Msg] to all nodes of a type via UDP
    fn beemsg_send_notifications<M: Msg + Serializable>(
        &self,
        node_types: &'static [NodeType],
        msg: &M,
    ) -> impl Future<Output = ()> + Send;

    /// Replace all stored BeeMsg network addresses of a node in the store
    fn beemsg_replace_node_addrs(&self, node_uid: Uid, new_addrs: impl Into<Arc<[SocketAddr]>>);

    // Run state

    /// Check if management is in pre shutdown state
    fn rs_pre_shutdown(&self) -> bool;
    /// Notify the runtime control that a particular client pulled states of a particular node type
    fn rs_notify_client_pulled_state(&self, node_type: NodeType, node_id: NodeId);

    // Licensing control

    /// Load and verify a license certificate
    fn lic_load_and_verify_cert(
        &self,
        cert_path: &Path,
    ) -> impl Future<Output = Result<String>> + Send;

    /// Get license certificate data
    fn lic_get_cert_data(&self) -> Result<GetCertDataResult>;

    /// Get licensed number of machines
    fn lic_get_num_machines(&self) -> Result<u32>;

    /// Verify a feature is licensed
    fn lic_verify_feature(&self, feature: LicensedFeature) -> Result<()>;
}
