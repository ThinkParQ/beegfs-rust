//! The BeeGFS managment service

#![feature(test)]
#![feature(fs_try_exists)]
#![feature(iterator_try_collect)]
#![feature(try_blocks)]
#![feature(slice_group_by)]

pub mod config;
mod context;
pub mod db;
mod grpc;
mod msg;
mod quota;
mod timer;
mod types;

use crate::config::Config;
use crate::context::Context;
use anyhow::Result;
use shared::conn::{incoming, Pool};
use shared::shutdown::Shutdown;
use shared::types::AuthenticationSecret;
use shared::NetworkAddr;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;

/// Contains information that is obtained at the start of the app and then never changes again.
#[derive(Debug)]
pub struct StaticInfo {
    pub user_config: Config,
    pub auth_secret: Option<AuthenticationSecret>,
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
pub async fn start(info: StaticInfo, shutdown: Shutdown) -> Result<()> {
    // Initialization

    // Static configuration which doesn't change at runtime
    let info = Box::leak(Box::new(info));

    let db = db::Connection::open(info.user_config.db_file.as_path()).await?;

    // UDP socket for in- and outgoing messages
    let udp_socket = Arc::new(
        UdpSocket::bind(SocketAddr::new(
            "0.0.0.0".parse()?,
            info.user_config.beegfs_port,
        ))
        .await?,
    );

    // Node address store and connection pool
    let conn_pool = Pool::new(
        udp_socket.clone(),
        info.user_config.connection_limit,
        info.auth_secret,
    );

    // Fill node addrs store from db
    db.op(db::node_nic::get_all_addrs)
        .await?
        .into_iter()
        .for_each(|a| conn_pool.replace_node_addrs(a.0, a.1));

    // Combines all handles for sharing between tasks
    let ctx = Context::new(conn_pool, db, info);

    // Listen for incoming TCP connections
    incoming::listen_tcp(
        SocketAddr::new("0.0.0.0".parse()?, ctx.info.user_config.beegfs_port),
        ctx.clone(),
        info.auth_secret.is_some(),
        shutdown.clone(),
    )
    .await?;

    // Recv UDP datagrams
    incoming::recv_udp(udp_socket, ctx.clone(), shutdown.clone())?;

    // Run the timers
    timer::start_tasks(ctx.clone(), shutdown.clone());

    // Start gRPC service
    grpc::serve(ctx, shutdown)?;

    Ok(())
}
