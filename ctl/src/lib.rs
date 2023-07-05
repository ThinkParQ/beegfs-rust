#![feature(test)]

pub mod config;

use crate::config::Config::*;
use crate::config::StaticConfig;
use anyhow::{anyhow, Result};
use shared::conn::msg_dispatch::*;
use shared::conn::{ConnPool, ConnPoolActor, ConnPoolConfig, SocketAddrResolver};
use shared::{shutdown, NodeTypeServer};

#[derive(Clone, Debug)]
struct EmptyMsgHandler {}

#[async_trait::async_trait]
impl DispatchRequest for EmptyMsgHandler {
    async fn dispatch_request(&self, _req: impl Request) -> Result<()> {
        Err(anyhow!("Unexpected incoming stream request"))
    }
}

pub async fn run(static_config: StaticConfig) -> Result<()> {
    let (conn_pool_actor, conn) = ConnPoolActor::new(ConnPoolConfig {
        stream_auth_secret: static_config.auth_secret,
        addr_resolver: SocketAddrResolver {},
        udp_sockets: vec![],
        tcp_listeners: vec![],
    });

    let (shutdown, _shutdown_control) = shutdown::new();

    conn_pool_actor.start_tasks(EmptyMsgHandler {}, shutdown);

    CommandExecutor {
        _conn: conn,
        static_config,
    }
    .execute()
    .await
}

struct CommandExecutor {
    _conn: ConnPool<SocketAddrResolver>,
    static_config: StaticConfig,
}

impl CommandExecutor {
    async fn execute(&self) -> Result<()> {
        use crate::config::Commands::*;
        match &self.static_config.args.command {
            Quota(cmd) => {
                use crate::config::Quota::*;
                match cmd {
                    Enable => {
                        unimplemented!()
                    }
                    Disable => unimplemented!(),
                    SetUpdateInterval { interval_secs: _ } => {
                        unimplemented!()
                    }
                }
            }
            Config(cmd) => match cmd {
                Get { key: _ } => {
                    unimplemented!()
                }
            },
            CapPools(cmd) => match cmd {
                config::CapPools::Set {
                    node_type,
                    inode_low_limit: _,
                    inode_emergency_limit: _,
                    space_low_limit: _,
                    space_emergency_limit: _,
                } => {
                    // let mut limits = match node_type {
                    //     NodeTypeServer::Meta => unimplemented!(),
                    //     NodeTypeServer::Storage => unimplemented!(),
                    // };

                    // limits.inodes_low = inode_low_limit.unwrap_or(limits.inodes_low);
                    // limits.inodes_emergency =
                    //     inode_emergency_limit.unwrap_or(limits.inodes_emergency);
                    // limits.space_low = space_low_limit.unwrap_or(limits.space_low);
                    // limits.space_emergency =
                    //     space_emergency_limit.unwrap_or(limits.space_emergency);

                    match node_type {
                        NodeTypeServer::Meta => unimplemented!(),
                        NodeTypeServer::Storage => {
                            unimplemented!()
                        }
                    }
                }
            },
        }
    }
}
