//! The BeeGFS managment service

#![feature(test)]
#![feature(fs_try_exists)]
#![feature(iterator_try_collect)]

mod app_context;
pub mod config;
pub mod db;
mod msg;
mod quota;
mod timer;

use crate::app_context::AppHandles;
use crate::config::{DynamicConfigArgs, StaticConfig};
use anyhow::Result;
use config::ConfigCache;
use shared::conn::{ConnPool, ConnPoolActor, ConnPoolConfig};
use shared::shutdown::Shutdown;
use shared::{AuthenticationSecret, Nic};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, UdpSocket};

pub(crate) type MgmtdPool = ConnPool<db::Connection>;

/// Contains information that is obtained at the start of the app and then never changes again.
#[derive(Debug)]
pub struct StaticInfo {
    pub static_config: StaticConfig,
    pub auth_secret: Option<AuthenticationSecret>,
    pub network_interfaces: Vec<Nic>,
}

/// Starts the management service.
///
/// Opens the necessary connections and starts all the tasks that provide the functionality of this
/// program. This is supposed to be called by the binary in main.rs, but could also be run in a
/// testing context without involving a standalone binary.
///
/// # Return behavior
/// Returns after all startup work is done. The caller is responsible for keeping the shutdown
/// control handle and use it to send a shutdown request when the program shall be terminated.
pub async fn start(
    static_config: StaticConfig,
    dynamic_config: Option<DynamicConfigArgs>,
    auth_secret: Option<AuthenticationSecret>,
    shutdown: Shutdown,
) -> Result<()> {
    // Initialization

    let network_interfaces = shared::network_interfaces(static_config.interfaces.as_slice())?;

    let static_info = Box::leak(Box::new(StaticInfo {
        static_config,
        auth_secret,
        network_interfaces,
    }));

    let db = db::Connection::open(static_info.static_config.db_file.as_path()).await?;

    // Apply the configuration from the dynamic config file to the database
    if let Some(c) = dynamic_config {
        c.apply_to_db(&db).await?;

        log::info!("Set system configuration from dynamic config file")
    }

    let config_cache = ConfigCache::from_db(db.clone()).await?;

    // Create the conn pool and bind sockets
    let (conn_pool_actor, conn) = ConnPoolActor::new(ConnPoolConfig {
        stream_auth_secret: static_info.auth_secret,
        udp_sockets: vec![Arc::new(
            UdpSocket::bind(SocketAddr::new(
                "0.0.0.0".parse()?,
                static_info.static_config.port.into(),
            ))
            .await?,
        )],
        tcp_listeners: vec![
            TcpListener::bind(SocketAddr::new(
                "0.0.0.0".parse()?,
                static_info.static_config.port.into(),
            ))
            .await?,
        ],
        addr_resolver: db.clone(),
    });

    // THe handles struct that combines all handles for sharing between tasks
    let app_handles = AppHandles::new(conn, db, config_cache, static_info);

    // Start tasks
    conn_pool_actor.start_tasks(app_handles.clone(), shutdown.clone());
    timer::start_tasks(app_handles, shutdown);

    Ok(())
}
