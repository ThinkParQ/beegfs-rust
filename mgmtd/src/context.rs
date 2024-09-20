//! Interfaces and implementations for in-app interaction between tasks or threads.

use crate::bee_msg::dispatch_request;
use crate::license::LicenseVerifier;
use crate::{ClientPulledStateNotification, StaticInfo};
use anyhow::Result;
use shared::conn::msg_dispatch::*;
use shared::conn::Pool;
use shared::run_state::WeakRunStateHandle;
use shared::types::{NodeId, NodeType};
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::mpsc;

/// A collection of Handles used for interacting and accessing the different components of the app.
///
/// This is the actual runtime object that can be shared between tasks. Interfaces should, however,
/// accept any implementation of the AppContext trait instead.
#[derive(Clone, Debug)]
pub(crate) struct Context(Arc<InnerContext>);

/// Stores the actual handles.
#[derive(Debug)]
pub(crate) struct InnerContext {
    pub conn: Pool,
    pub db: tokio_rusqlite::Connection,
    pub license: LicenseVerifier,
    pub info: &'static StaticInfo,
    pub run_state: WeakRunStateHandle,
    shutdown_client_id: mpsc::Sender<ClientPulledStateNotification>,
}

impl Context {
    /// Creates a new AppHandles object.
    ///
    /// Takes all the stored handles.
    pub(crate) fn new(
        conn: Pool,
        db: tokio_rusqlite::Connection,
        license: LicenseVerifier,
        info: &'static StaticInfo,
        run_state: WeakRunStateHandle,
        shutdown_client_id: mpsc::Sender<ClientPulledStateNotification>,
    ) -> Self {
        Self(Arc::new(InnerContext {
            conn,
            db,
            license,
            info,
            run_state,
            shutdown_client_id,
        }))
    }

    pub(crate) fn notify_client_pulled_state(&self, node_type: NodeType, node_id: NodeId) {
        if self.run_state.pre_shutdown() {
            let tx = self.shutdown_client_id.clone();

            // We don't want to block the task calling this and are not interested by the results
            tokio::spawn(async move {
                let _ = tx.send((node_type, node_id)).await;
            });
        }
    }
}

/// Derefs to InnerAppHandle which stores all the handles.
///
/// Allows transparent access.
impl Deref for Context {
    type Target = InnerContext;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DispatchRequest for Context {
    async fn dispatch_request(&self, req: impl Request) -> Result<()> {
        dispatch_request(self, req).await
    }
}
