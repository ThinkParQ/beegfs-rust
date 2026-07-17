use super::*;
use crate::ClientPulledStateNotification;
use crate::bee_msg::dispatch_request;
use crate::license::LicenseVerifier;
use crate::types::SqliteEnumExt;
use anyhow::Result;
use protobuf::license::GetCertDataResult;
use rusqlite::{Connection, Transaction};
use shared::conn::msg_dispatch::{DispatchRequest, Request};
use shared::conn::outgoing::Pool;
use shared::peak_concurrency_tracker;
use shared::run_state::WeakRunStateHandle;
use sqlite::{Connections, TransactionExt, rarray_param};
use sqlite_check::sql;
use std::fmt::Debug;
use std::ops::Deref;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::mpsc;
use tokio::time::Instant;

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
        if node_types.is_empty() {
            return;
        }

        static NOTIFICATION_COUNTER: AtomicU32 = AtomicU32::new(0);
        let trace_data = if log::log_enabled!(log::Level::Trace) {
            let start_time = Instant::now();
            let count = NOTIFICATION_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
            log::trace!("NOTIFICATION #{count} to {node_types:?}: {msg:?}");
            Some((count, start_time))
        } else {
            None
        };

        // We want to track the concurrent notification tasks that are going on to make it easy
        // to catch a potential bottleneck here on big systems.
        let concurrency_tracker = peak_concurrency_tracker!();
        if let Some(peak) = concurrency_tracker.peaked() {
            log::info!("Concurrent notifications peaked at {peak}",);
        }

        let mut node_count = 0;
        if let Err(err) = async {
            let nodes: Vec<Uid> = self
                .read_tx(move |tx| {
                    Ok(tx.query_map_collect(
                        sql!("SELECT node_uid FROM nodes WHERE node_type IN rarray(?1)"),
                        [rarray_param(node_types.iter().map(|e| e.sql_variant()))],
                        |row| row.get::<_, Uid>(0),
                    )?)
                })
                .await?;

            node_count = nodes.len();

            self.conn.broadcast_datagram(nodes.into_iter(), msg).await?;

            Ok(()) as Result<_>
        }
        .await
        {
            log::error!("Notification could not be sent: {err:#}");
        }

        if let Some(td) = trace_data {
            log::trace!(
                "-> Notification #{} to {node_count} nodes completed after {:?}",
                td.0,
                td.1.elapsed()
            );
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

    async fn load_and_verify_license_cert(
        &self,
        cert_path: &Path,
        prev_trial_serial: Option<&str>,
    ) -> Result<String> {
        LicenseVerifier::load_and_verify_license_cert(&self.license, cert_path, prev_trial_serial)
            .await
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
