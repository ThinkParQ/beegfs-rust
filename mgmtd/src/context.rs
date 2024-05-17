//! Interfaces and implementations for in-app interaction between tasks or threads.

use crate::bee_msg::dispatch_request;
use crate::StaticInfo;
use anyhow::Result;
use shared::conn::msg_dispatch::*;
use shared::conn::Pool;
use std::ops::Deref;
use std::sync::Arc;

/// A collection of Handles used for interacting and accessing the different components of the app.
///
/// This is the actual runtime object that can be shared between tasks. Interfaces should, however,
/// accept any implementation of the AppContext trait instead.
#[derive(Clone, Debug)]
pub(crate) struct Context {
    /// Stores the actual values.
    ///
    /// Wrapped in an Arc since AppHandles is meant to be shared between threads.
    inner: Arc<InnerContext>,
}

/// Stores the actual handles.
#[derive(Debug)]
pub(crate) struct InnerContext {
    pub conn: Pool,
    pub db: tokio_rusqlite::Connection,
    pub info: &'static StaticInfo,
}

impl Context {
    /// Creates a new AppHandles object.
    ///
    /// Takes all the stored handles.
    pub(crate) fn new(
        conn: Pool,
        db: tokio_rusqlite::Connection,
        info: &'static StaticInfo,
    ) -> Self {
        Self {
            inner: Arc::new(InnerContext { conn, db, info }),
        }
    }
}

/// Derefs to InnerAppHandle which stores all the handles.
///
/// Allows transparent access.
impl Deref for Context {
    type Target = InnerContext;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DispatchRequest for Context {
    async fn dispatch_request(&self, req: impl Request) -> Result<()> {
        dispatch_request(self, req).await
    }
}
