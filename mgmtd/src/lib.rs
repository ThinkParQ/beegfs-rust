//! The BeeGFS management service

mod bee_msg;
mod cap_pool;
pub mod config;
mod context;
pub mod db;
mod error;
mod grpc;
pub mod license;
mod quota;
mod timer;
mod types;

use crate::config::Config;
use crate::context::Context;
use anyhow::Result;
use bee_msg::notify_nodes;
use db::node_nic::ReplaceNic;
use license::LicenseVerifier;
use shared::bee_msg::target::RefreshTargetStates;
use shared::conn::{incoming, Pool};
use shared::run_state::{self, RunStateControl};
use shared::types::{AuthSecret, NicType, NodeId, NodeType, MGMTD_UID};
use shared::NetworkAddr;
use sqlite::{ConnectionExt, TransactionExt};
use sqlite_check::sql;
use std::collections::HashSet;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::Instant;
use types::SqliteEnumExt;

/// Contains information that is obtained at the start of the app and then never changes again.
#[derive(Debug)]
pub struct StaticInfo {
    pub user_config: Config,
    pub auth_secret: Option<AuthSecret>,
    pub network_addrs: Vec<NetworkAddr>,
}

/// Starts the management service.
///
/// Opens the necessary connections and starts all the tasks that provide the functionality of this
/// program. This is supposed to be called by the binary in main.rs, but could also be run in a
/// testing context without involving a standalone binary.
///
/// # Return behavior
/// Returns after all setup work is done and all tasks are started. The caller is responsible for
/// keeping the shutdown control handle and send a shutdown request when the program shall
/// be terminated.
pub async fn start(info: StaticInfo, license: LicenseVerifier) -> Result<RunControl> {
    // Initialization

    let (run_state, run_state_control) = run_state::new();

    // Static configuration which doesn't change at runtime
    let info = Box::leak(Box::new(info));

    // UDP socket for in- and outgoing messages
    let udp_socket = Arc::new(
        UdpSocket::bind(SocketAddr::new(
            "0.0.0.0".parse()?,
            info.user_config.beemsg_port,
        ))
        .await?,
    );

    // Node address store and connection pool
    let conn_pool = Pool::new(
        udp_socket.clone(),
        info.user_config.connection_limit,
        info.auth_secret,
    );

    let mut db = sqlite::open_async(info.user_config.db_file.as_path()).await?;
    sqlite::check_schema_async(&mut db, db::MIGRATIONS).await?;

    log::info!(
        "Opened database at {:?}",
        info.user_config.db_file.as_path()
    );

    db.op(|tx| {
        // Update management node entry in db
        db::node::update(tx, MGMTD_UID, info.user_config.beemsg_port, None)?;

        // Update management nics entry in db
        db::node_nic::replace(
            tx,
            MGMTD_UID,
            info.network_addrs.iter().map(|e| ReplaceNic {
                nic_type: NicType::Ethernet,
                addr: &e.addr,
                name: &e.name,
            }),
        )
    })
    .await?;

    // Fill node addrs store from db
    db.op(db::node_nic::get_all_addrs)
        .await?
        .into_iter()
        .for_each(|a| conn_pool.replace_node_addrs(a.0, a.1));

    // This is used to signal a client that pulled its state back to the RunControl
    let (shutdown_client_tx, shutdown_client_rx) = mpsc::channel(16);

    // Combines all handles for sharing between tasks
    let ctx = Context::new(
        conn_pool,
        db,
        license,
        info,
        run_state.clone_weak(),
        shutdown_client_tx,
    );

    // Listen for incoming TCP connections
    incoming::listen_tcp(
        SocketAddr::new("0.0.0.0".parse()?, ctx.info.user_config.beemsg_port),
        ctx.clone(),
        info.auth_secret.is_some(),
        run_state.clone(),
    )
    .await?;

    // Recv UDP datagrams
    incoming::recv_udp(udp_socket, ctx.clone(), run_state.clone())?;

    // Run the timers
    timer::start_tasks(ctx.clone(), run_state.clone());

    // Start gRPC service
    grpc::serve(ctx.clone(), run_state)?;

    Ok(RunControl {
        ctx: ctx.clone(),
        run_state_control,
        shutdown_client_rx,
    })
}

/// Controls the running application.
#[derive(Debug)]
pub struct RunControl {
    ctx: Context,
    run_state_control: RunStateControl,
    shutdown_client_rx: mpsc::Receiver<ClientPulledStateNotification>,
}

/// Represents a client having pulled meta or storage states. Clients pull them separately, and to
/// skip the wait on shutdown we need to be sure that both have been received.
pub type ClientPulledStateNotification = (NodeType, NodeId);

impl RunControl {
    /// Waits for the provided future to complete before initiating shutdown. Completes after
    /// shutdown is done.
    pub async fn wait_for_shutdown<F, R>(mut self, shutdown_signal: F)
    where
        F: Fn() -> R,
        R: Future,
    {
        log::warn!("Waiting for shutdown signal ...");
        shutdown_signal().await;
        log::warn!("Received shutdown signal");

        // Set pre shutdown state to freeze the relevant system state - message handlers that do
        // modify e.g. target states should now deny change.
        self.run_state_control.pre_shutdown();

        let client_list: HashSet<ClientPulledStateNotification> = self
            .ctx
            .db
            .op(move |tx| {
                let buddy_groups: i64 =
                    tx.query_row(sql!("SELECT COUNT(*) FROM buddy_groups"), [], |row| {
                        row.get(0)
                    })?;

                if buddy_groups == 0 {
                    return Ok(HashSet::new());
                }

                // Build the client list as a cartesian product `client_id x node_type` as each
                // client updates its state separately for meta and storage and we
                // have to wait until both have been pulled.
                let clients = tx.query_map_collect(
                    sql!(
                        "SELECT n.node_id, t.node_type FROM client_nodes AS n
                        CROSS JOIN node_types AS t
                        WHERE t.name IN ('meta', 'storage')"
                    ),
                    [],
                    |row| Ok((NodeType::from_row(row, 1)?, row.get(0)?)),
                )?;

                Ok(clients)
            })
            .await
            .unwrap_or_default();

        // We only need to wait in pre shutdown if there are clients mounted AND buddy groups exist
        // in the system. Otherwise, nothing bad can happen.
        if !client_list.is_empty() {
            log::warn!(
                "Buddy groups are in use and clients are registered - \
                waiting for all clients to pull state (timeout after {:?}) ...",
                self.ctx.info.user_config.node_offline_timeout
            );

            // Let the nodes pull the new states as soon as possible
            notify_nodes(
                &self.ctx,
                &[NodeType::Client, NodeType::Meta, NodeType::Storage],
                &RefreshTargetStates {
                    ack_id: b"".to_vec(),
                },
            )
            .await;

            tokio::select! {
                // Wait for all clients having downloaded the state
                _ = self.wait_for_clients(client_list) => {}
                // or wait for another shutdown signal
                _ = shutdown_signal() => {}
            }
        }

        log::warn!("Waiting for all tasks to complete ... ");

        tokio::select! {
            // Wait for all tasks dropping the RunState handles
            _ = self.run_state_control.shutdown() => {
                log::warn!("Shutdown completed");
            }
            // or wait for another shutdown signal
            _ = shutdown_signal() => {
                log::warn!("Shutdown forced");
            }
        }
    }

    /// Waits until every client in `client_list` has been received to the `self.shutdown_client_`
    async fn wait_for_clients(&mut self, mut client_list: HashSet<ClientPulledStateNotification>) {
        let deadline = Instant::now() + self.ctx.info.user_config.node_offline_timeout;

        let receive_client_ids = async {
            while let Some(client_id) = self.shutdown_client_rx.recv().await {
                client_list.remove(&client_id);

                if client_list.is_empty() {
                    break;
                }
            }
        };

        tokio::select! {
            // Wait for all clients having downloaded the state
            _ = receive_client_ids => {}
            // or wait for the deadline
            _ = tokio::time::sleep_until(deadline) => {}
        }

        // If the receive loop exited due to error (e.g. all senders being dropped), we just wait
        if !client_list.is_empty() {
            tokio::time::sleep_until(deadline).await;
        }
    }
}
