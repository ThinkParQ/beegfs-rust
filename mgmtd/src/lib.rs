#![feature(test)]
#![feature(fs_try_exists)]
#![feature(iterator_try_collect)]
#![feature(try_blocks)]

mod component_handles;
pub mod config;
pub mod db;
mod msg;
mod notification;
mod timer;

use crate::component_handles::ComponentHandles;
use crate::config::{Config, RuntimeConfig};
use anyhow::Result;
use shared::conn::{ConnPool, ConnPoolActor, ConnPoolConfig};
use shared::shutdown::Shutdown;
use shared::{AuthenticationSecret, Nic};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, UdpSocket};

pub(crate) type MgmtdPool = ConnPool<db::Handle>;

#[derive(Debug)]
pub struct StaticInfo {
    pub static_config: Config,
    pub auth_secret: Option<AuthenticationSecret>,
    pub network_interfaces: Vec<Nic>,
}

pub async fn start(
    static_config: Config,
    runtime_config: Option<RuntimeConfig>,
    auth_secret: Option<AuthenticationSecret>,
    shutdown: Shutdown,
) -> Result<()> {
    // init

    let network_interfaces = shared::network_interfaces(static_config.interfaces.as_slice())?;

    let static_info = Box::leak(Box::new(StaticInfo {
        static_config,
        auth_secret,
        network_interfaces,
    }));

    let db = db::Handle::open(static_info.static_config.db_file.as_path()).await?;

    if let Some(c) = &runtime_config {
        c.apply_to_db(&db).await?;

        log::info!("Set system configuration from supplied data")
    }

    let (config_input, config) = ::config::from_source(db.clone()).await?;

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

    // run tasks

    conn_pool_actor.start_tasks(
        ComponentHandles {
            conn: conn.clone(),
            db: db.clone(),
            static_config: static_info,
            config_input,
            config: config.clone(),
        },
        shutdown.clone(),
    );

    timer::start_tasks(db, conn, config, shutdown);

    Ok(())
}
