use clap::{Parser, Subcommand};
use shared::*;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug)]
pub struct StaticConfig {
    pub args: CmdLineArgs,
    pub auth_secret: Option<AuthenticationSecret>,
    pub management_addr: SocketAddr,
}

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct CmdLineArgs {
    /// Authentication file location
    #[clap(long)]
    pub auth_file: Option<PathBuf>,
    /// The command to execute
    #[clap(subcommand, name = "COMMAND")]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Quota configuration
    #[clap(subcommand)]
    Quota(Quota),
    /// System configuration
    #[clap(subcommand)]
    Config(Config),
    /// Capacity pool management
    #[clap(subcommand)]
    CapPools(CapPools),
}

#[derive(Subcommand, Debug)]
pub enum Quota {
    Enable,
    Disable,
    /// Sets the exceeded quota updatew interval
    SetUpdateInterval {
        #[clap(value_parser)]
        interval_secs: u64,
    },
}

#[derive(Subcommand, Debug)]
pub enum Config {
    /// Print config values
    Get {
        #[clap(value_parser)]
        /// Filter output by config key (case insensitive)
        key: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum CapPools {
    /// Set capacity pool limits
    Set {
        /// The node type to set the limit for
        node_type: NodeTypeServer,
        #[clap(long)]
        /// The inode low limit
        inode_low_limit: Option<u64>,
        #[clap(long)]
        /// The inode emergency limit
        inode_emergency_limit: Option<u64>,
        #[clap(long)]
        /// The space low limit in bytes
        space_low_limit: Option<u64>,
        #[clap(long)]
        /// The space emergency limit in bytes
        space_emergency_limit: Option<u64>,
    },
}
