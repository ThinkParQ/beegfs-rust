use super::*;
use crate::ClientPulledStateNotification;
use crate::bee_msg::dispatch_request;
use crate::license::LicenseVerifier;
use anyhow::Result;
use protobuf::license::GetCertDataResult;
use rusqlite::{Connection, Transaction};
use shared::conn::msg_dispatch::{DispatchRequest, Request};
use shared::conn::outgoing::Pool;
use shared::run_state::WeakRunStateHandle;
use sqlite::Connections;
use std::fmt::Debug;
use std::ops::Deref;
use tokio::sync::mpsc;

/// A collection of Handles used for interacting and accessing the different components of the app.
///
/// This is the actual runtime object that can be shared between tasks. Interfaces should, however,
/// accept any implementation of the AppContext trait instead.
#[derive(Clone, Debug)]
pub(crate) struct RuntimeApp(Arc<InnerAppHandles>);

/// Stores the actual handles.
#[derive(Debug)]
pub(crate) struct InnerAppHandles {
    pub conn: Pool,
    pub db: Connections,
    pub license: LicenseVerifier,
    pub info: &'static StaticInfo,
    pub run_state: WeakRunStateHandle,
    shutdown_client_id: mpsc::Sender<ClientPulledStateNotification>,
}

impl RuntimeApp {
    /// Creates a new AppHandles object.
    ///
    /// Takes all the stored handles.
    pub(crate) fn new(
        conn: Pool,
        db: Connections,
        license: LicenseVerifier,
        info: &'static StaticInfo,
        run_state: WeakRunStateHandle,
        shutdown_client_id: mpsc::Sender<ClientPulledStateNotification>,
    ) -> Self {
        Self(Arc::new(InnerAppHandles {
            conn,
            db,
            license,
            info,
            run_state,
            shutdown_client_id,
        }))
    }
}

/// Derefs to InnerAppHandle which stores all the handles.
///
/// Allows transparent access.
impl Deref for RuntimeApp {
    type Target = InnerAppHandles;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Adds BeeMsg dispatching functionality to AppHandles
impl DispatchRequest for RuntimeApp {
    async fn dispatch_request(&self, req: impl Request) -> Result<()> {
        dispatch_request(self, req).await
    }
}

impl App for RuntimeApp {
    fn static_info(&self) -> &StaticInfo {
        self.info
    }

    async fn read_tx<T: Send + 'static + FnOnce(&Transaction) -> Result<R>, R: Send + 'static>(
        &self,
        op: T,
    ) -> Result<R> {
        Connections::read_tx(&self.db, op).await
    }

    async fn write_tx<T: Send + 'static + FnOnce(&Transaction) -> Result<R>, R: Send + 'static>(
        &self,
        op: T,
    ) -> Result<R> {
        Connections::write_tx(&self.db, op).await
    }

    async fn write_tx_no_sync<
        T: Send + 'static + FnOnce(&Transaction) -> Result<R>,
        R: Send + 'static,
    >(
        &self,
        op: T,
    ) -> Result<R> {
        Connections::write_tx_no_sync(&self.db, op).await
    }

    async fn db_conn<
        T: Send + 'static + FnOnce(&mut Connection) -> Result<R>,
        R: Send + 'static,
    >(
        &self,
        op: T,
    ) -> Result<R> {
        Connections::conn(&self.db, op).await
    }

    async fn request<M: Msg + Serializable, R: Msg + Deserializable>(
        &self,
        node_uid: Uid,
        msg: &M,
    ) -> Result<R> {
        Pool::request(&self.conn, node_uid, msg).await
    }

    async fn send_notifications<M: Msg + Serializable>(
        &self,
        node_types: &'static [NodeType],
        msg: &M,
    ) {
        log::trace!("NOTIFICATION to {node_types:?}: {msg:?}");

        for t in node_types {
            if let Err(err) = async {
                let nodes = self
                    .read_tx(move |tx| crate::db::node::get_with_type(tx, *t))
                    .await?;

                self.conn
                    .broadcast_datagram(nodes.into_iter().map(|e| e.uid), msg)
                    .await?;

                Ok(()) as Result<_>
            }
            .await
            {
                log::error!("Notification could not be sent to all {t} nodes: {err:#}");
            }
        }
    }

    fn replace_node_addrs(&self, node_uid: Uid, new_addrs: impl Into<Arc<[SocketAddr]>>) {
        Pool::replace_node_addrs(&self.conn, node_uid, new_addrs)
    }

    fn is_pre_shutdown(&self) -> bool {
        WeakRunStateHandle::pre_shutdown(&self.run_state)
    }

    fn notify_client_pulled_state(&self, node_type: NodeType, node_id: NodeId) {
        if self.run_state.pre_shutdown() {
            let tx = self.shutdown_client_id.clone();

            // We don't want to block the task calling this and are not interested by the results
            tokio::spawn(async move {
                let _ = tx.send((node_type, node_id)).await;
            });
        }
    }

    async fn load_and_verify_license_cert(&self, cert_path: &Path) -> Result<String> {
        LicenseVerifier::load_and_verify_license_cert(&self.license, cert_path).await
    }

    fn get_license_cert_data(&self) -> Result<GetCertDataResult> {
        LicenseVerifier::get_license_cert_data(&self.license)
    }

    fn get_licensed_machines(&self) -> Result<u32> {
        LicenseVerifier::get_licensed_machines(&self.license)
    }

    fn verify_licensed_feature(&self, feature: LicensedFeature) -> Result<()> {
        self.license.verify_licensed_feature(feature)
    }
}
