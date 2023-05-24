#![feature(test)]

pub mod config;

use crate::config::Config::*;
use crate::config::StaticConfig;
use ::config::{Cache, GenericConfigValue};
use anyhow::Result;
use shared::config::*;
use shared::conn::msg_dispatch::*;
use shared::conn::{ConnPool, ConnPoolActor, ConnPoolConfig, PeerID, SocketAddrResolver};
use shared::msg::{self, SetConfigResp};
use shared::{shutdown, NodeTypeServer};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
struct EmptyMsgHandler {}

#[async_trait::async_trait]
impl DispatchRequest for EmptyMsgHandler {
    async fn dispatch_request(
        &mut self,
        _req: impl RequestConnectionController + DeserializeMsg,
    ) -> Result<()> {
        unreachable!("Unexpected incoming stream request");
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

    let (_, config) = ::config::from_source(ManagementSource::new(
        PeerID::Addr(static_config.management_addr),
        conn.clone(),
    ))
    .await?;

    CommandExecutor {
        conn,
        static_config,
        config,
    }
    .execute()
    .await
}

struct CommandExecutor {
    conn: ConnPool<SocketAddrResolver>,
    static_config: StaticConfig,
    config: Cache<BeeConfig>,
}

impl CommandExecutor {
    async fn execute(&self) -> Result<()> {
        use crate::config::Commands::*;
        match &self.static_config.args.command {
            Quota(cmd) => {
                use crate::config::Quota::*;
                match cmd {
                    Enable => self.set_config::<QuotaEnable>(true).await,
                    Disable => self.set_config::<QuotaEnable>(false).await,
                    AddUserID { id } => {
                        let mut ids = self.config.get::<QuotaUserIDs>();
                        ids.insert((*id).into());
                        self.set_config::<QuotaUserIDs>(ids).await
                    }
                    AddGroupID { id } => {
                        let mut ids = self.config.get::<QuotaGroupIDs>();
                        ids.insert((*id).into());
                        self.set_config::<QuotaGroupIDs>(ids).await
                    }
                    RemoveUserID { id } => {
                        let mut ids = self.config.get::<QuotaUserIDs>();
                        ids.retain(|e| e.as_ref() != id);
                        self.set_config::<QuotaUserIDs>(ids).await
                    }
                    RemoveGroupID { id } => {
                        let mut ids = self.config.get::<QuotaGroupIDs>();
                        ids.retain(|e| e.as_ref() != id);
                        self.set_config::<QuotaGroupIDs>(ids).await
                    }
                    SetUpdateInterval { interval_secs } => {
                        self.set_config::<QuotaUpdateInterval>(std::time::Duration::from_secs(
                            *interval_secs,
                        ))
                        .await
                    }
                }
            }
            Config(cmd) => match cmd {
                Get { ref key } => {
                    let cache_map = self.config.borrow_all();

                    let cache_map: BTreeMap<&String, &Box<dyn GenericConfigValue>> = cache_map
                        .iter()
                        .filter(|(k, _)| {
                            if let Some(key) = key {
                                k.to_lowercase().contains(&key.to_lowercase())
                            } else {
                                true
                            }
                        })
                        .collect();

                    println!("{cache_map:#?}");
                    Ok(())
                }
            },
            CapPools(cmd) => match cmd {
                config::CapPools::Set {
                    node_type,
                    inode_low_limit,
                    inode_emergency_limit,
                    space_low_limit,
                    space_emergency_limit,
                } => {
                    let mut limits = match node_type {
                        NodeTypeServer::Meta => self.config.get::<CapPoolMetaLimits>(),
                        NodeTypeServer::Storage => self.config.get::<CapPoolStorageLimits>(),
                    };

                    limits.inodes_low = inode_low_limit.unwrap_or(limits.inodes_low);
                    limits.inodes_emergency =
                        inode_emergency_limit.unwrap_or(limits.inodes_emergency);
                    limits.space_low = space_low_limit.unwrap_or(limits.space_low);
                    limits.space_emergency =
                        space_emergency_limit.unwrap_or(limits.space_emergency);

                    match node_type {
                        NodeTypeServer::Meta => self.set_config::<CapPoolMetaLimits>(limits).await,
                        NodeTypeServer::Storage => {
                            self.set_config::<CapPoolStorageLimits>(limits).await
                        }
                    }
                }
            },
        }
    }

    async fn set_config<T: ::config::Field>(&self, value: T::Value) -> Result<()> {
        let _ = self
            .conn
            .request::<_, SetConfigResp>(
                PeerID::Addr(self.static_config.management_addr),
                &msg::SetConfig {
                    entries: [(T::KEY.to_string(), T::serialize(&value)?)].into(),
                },
            )
            .await?;

        Ok(())
    }
}
