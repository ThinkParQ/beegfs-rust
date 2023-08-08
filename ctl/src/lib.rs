#![feature(test)]

pub mod config;

use crate::config::Config::*;
use crate::config::StaticConfig;
use anyhow::Result;
use shared::NodeTypeServer;

pub async fn run(static_config: StaticConfig) -> Result<()> {
    CommandExecutor { static_config }.execute().await
}

struct CommandExecutor {
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
