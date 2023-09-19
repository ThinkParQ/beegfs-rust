//! The BeeGFS managment service

#![feature(test)]
#![feature(fs_try_exists)]
#![feature(iterator_try_collect)]
#![feature(try_blocks)]
#![feature(slice_group_by)]

pub mod config;
mod context;
pub mod db;
// mod grpc;
mod msg;
mod quota;
mod timer;
mod types;

use crate::config::Config;
use crate::context::Context;
use anyhow::Result;
use shared::conn::incoming::{listen_tcp, recv_udp};
use shared::conn::Pool;
use shared::shutdown::Shutdown;
use shared::types::AuthenticationSecret;
use shared::NetworkAddr;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, UdpSocket};

/// Contains information that is obtained at the start of the app and then never changes again.
#[derive(Debug)]
pub struct RuntimeInfo {
    pub config: Config,
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
pub async fn start(
    config: Config,
    auth_secret: Option<AuthenticationSecret>,
    shutdown: Shutdown,
) -> Result<()> {
    // Initialization

    let network_interfaces = shared::ethernet_interfaces(config.interfaces.as_slice())?;

    // Static configuration which doesn't change at runtime
    let info = Box::leak(Box::new(RuntimeInfo {
        config,
        auth_secret,
        network_addrs: network_interfaces,
    }));

    let db = db::Connection::open(info.config.db_file.as_path()).await?;

    // TCP listener for incoming connections
    let tcp_listener =
        TcpListener::bind(SocketAddr::new("0.0.0.0".parse()?, info.config.beegfs_port)).await?;

    // UDP socket for in- and outgoing messages
    let udp_socket = Arc::new(
        UdpSocket::bind(SocketAddr::new("0.0.0.0".parse()?, info.config.beegfs_port)).await?,
    );

    // Node address store and connection pool
    let conn_pool = Pool::new(
        udp_socket.clone(),
        info.config.connection_limit,
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
    tokio::spawn(listen_tcp(
        tcp_listener,
        ctx.clone(),
        info.auth_secret.is_some(),
        shutdown.clone(),
    ));

    // Recv UDP datagrams
    tokio::spawn(recv_udp(udp_socket, ctx.clone(), shutdown.clone()));

    // Run the timers
    timer::start_tasks(ctx.clone(), shutdown.clone());

    // Start gRPC service
    // tokio::spawn(grpc::serve(ctx, shutdown));

    Ok(())
}
