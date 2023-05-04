use crate::db;
use ::config::Field;
use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use log::LevelFilter;
use serde::{Deserialize, Serialize};
use shared::parser::integer_with_time_unit;
use shared::{config, CapPoolLimits, Port, QuotaID};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug)]
pub struct Config {
    pub init: bool,
    pub port: Port,
    pub interfaces: Vec<String>,
    pub db_file: PathBuf,
    pub auth_file: PathBuf,
    pub auth_enable: bool,
    pub log_target: LogTarget,
    pub log_level: LevelFilter,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            init: false,
            port: 8008.into(),
            interfaces: vec![],
            db_file: "/var/lib/beegfs/mgmtd.sqlite".into(),
            auth_file: "/etc/beegfs/mgmtd.auth".into(),
            auth_enable: true,
            log_target: LogTarget::Std,
            log_level: LevelFilter::Warn,
        }
    }
}

/// BeeGFS mgmtd Rust prototype
///
/// TODO
#[derive(Debug, Default, Parser, Deserialize)]
#[command(
    author,
    version,
    rename_all = "kebab-case",
    hide_possible_values = false
)]
struct ConfigArgs {
    //
    // CLI and config file args - can be filled in later from another ConfigArgs if they are
    // still none
    /// Sets the port (TCP and UDP) to listen on [default: 8008]
    #[arg(long, short = 'p')]
    port: Option<Port>,
    /// Network interfaces reported to other nodes for incoming communication
    ///
    /// Can be specified multiple times. If not given, all suitable interfaces
    /// can be used.
    #[arg(long = "interface", short = 'i')]
    interfaces: Option<Vec<String>>,
    /// Sqlite database file location [default: /var/lib/beegfs/mgmtd.sqlite]
    #[arg(long)]
    db_file: Option<PathBuf>,
    /// Enable authentication [default: true]
    #[arg(long)]
    auth_enable: Option<bool>,
    /// Authentication file location [default: /etc/beegfs/mgmtd.auth]
    #[arg(long)]
    auth_file: Option<PathBuf>,
    /// Log target [default: std]
    ///
    /// Sets the logging mechanism to use.
    #[arg(long)]
    log_target: Option<LogTarget>,
    /// Log level [default: warn]
    ///
    /// Sets the maximum level to log.
    ///
    /// When logging to std, the logging behavior can be fine controlled by
    /// setting the RUST_LOG environment variable. This overwrites this
    /// setting. See the env_logger documentation for more details:
    ///
    /// https://docs.rs/env_logger/latest/env_logger/#enabling-logging
    #[arg(long)]
    log_level: Option<LogLevel>,

    //
    // CLI only args - we do not parse them from file and also do not update them
    /// Initialialize a new installation, then quit
    #[arg(long)]
    #[serde(skip)]
    init: bool,
    /// Config file location [default: /etc/beegfs/mgmtd.toml]
    #[arg(
        long,
        default_value = "/etc/beegfs/mgmtd.toml",
        hide_default_value = true
    )]
    #[serde(skip)]
    config_file: Option<PathBuf>,
    /// [TEMPORARY] Runtime config file location [default:
    /// /etc/beegfs/mgmtd-runtime.toml]
    ///
    /// This option will be replaced with the new ctl tool when it is done.
    #[arg(
        long,
        default_value = "/etc/beegfs/mgmtd-runtime.toml",
        hide_default_value = true
    )]
    #[serde(skip)]
    runtime_config_file: Option<PathBuf>,
}

impl ConfigArgs {
    /// Fill None fields from another source - ignore Some(_) fields
    /// This means, what is put in first has higher priority
    fn fill_from(&mut self, other: Self) {
        if self.port.is_none() {
            self.port = other.port
        };

        if self.interfaces.is_none() {
            self.interfaces = other.interfaces
        };

        if self.db_file.is_none() {
            self.db_file = other.db_file
        };

        if self.auth_file.is_none() {
            self.auth_file = other.auth_file
        };

        if self.auth_enable.is_none() {
            self.auth_enable = other.auth_enable
        };

        if self.log_target.is_none() {
            self.log_target = other.log_target
        };

        if self.log_level.is_none() {
            self.log_level = other.log_level
        };
    }

    fn into_config(self) -> Config {
        let mut config = Config {
            init: self.init,
            ..Config::default()
        };

        config.port = self.port.unwrap_or(config.port);
        config.interfaces = self.interfaces.unwrap_or(config.interfaces);
        config.db_file = self.db_file.unwrap_or(config.db_file);
        config.auth_file = self.auth_file.unwrap_or(config.auth_file);
        config.auth_enable = self.auth_enable.unwrap_or(config.auth_enable);
        config.log_target = self.log_target.unwrap_or(config.log_target);
        if let Some(l) = self.log_level {
            config.log_level = l.into();
        }

        config
    }
}

pub fn load_and_parse() -> Result<(Config, Option<RuntimeConfig>)> {
    let mut args = ConfigArgs::parse();

    if let Some(ref file) = args.config_file {
        match std::fs::read_to_string(file) {
            Ok(ref toml_config) => {
                let file_args: ConfigArgs =
                    toml::from_str(toml_config).with_context(|| "Couldn't parse config file")?;

                println!("Loaded node configuration from {file:?}");

                args.fill_from(file_args);
            }
            Err(err) => {
                if file != Path::new("/etc/beegfs/mgmtd.toml") {
                    return Err(err)
                        .with_context(|| format!("Could not open config file at {file:?}"));
                }

                println!("No config file found at default location, ignoring");
            }
        }
    }

    let tmp_runtime_config = if let Some(ref file) = args.runtime_config_file {
        match std::fs::read_to_string(file) {
            Ok(ref toml_config) => {
                let config_values: RuntimeConfig = toml::from_str(toml_config)
                    .with_context(|| "Couldn't parse runtime config file")?;

                println!("Loaded runtime configuration from {file:?}");

                Some(config_values)
            }
            Err(err) => {
                if file != Path::new("/etc/beegfs/mgmtd-runtime.toml") {
                    return Err(err).with_context(|| {
                        format!("Could not open runtime config file at {file:?}")
                    });
                }

                println!("No runtime config file found at default location, ignoring");

                None
            }
        }
    } else {
        None
    };

    Ok((args.into_config(), tmp_runtime_config))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RuntimeConfig {
    registration_enable: bool,
    #[serde(with = "integer_with_time_unit")]
    node_offline_timeout: Duration,
    #[serde(with = "integer_with_time_unit")]
    client_auto_remove_timeout: Duration,
    quota_enable: bool,
    quota_user_ids: HashSet<QuotaID>,
    quota_group_ids: HashSet<QuotaID>,
    #[serde(with = "integer_with_time_unit")]
    quota_update_interval: Duration,
    cap_pool_meta_limits: CapPoolLimits,
    cap_pool_storage_limits: CapPoolLimits,
}

impl RuntimeConfig {
    pub async fn apply_to_db(&self, db: &db::Handle) -> anyhow::Result<()> {
        let entries = [
            (
                config::RegistrationEnable::KEY.into(),
                config::RegistrationEnable::serialize(&self.registration_enable)?,
            ),
            (
                config::NodeOfflineTimeout::KEY.into(),
                config::NodeOfflineTimeout::serialize(&self.node_offline_timeout)?,
            ),
            (
                config::ClientAutoRemoveTimeout::KEY.into(),
                config::ClientAutoRemoveTimeout::serialize(&self.client_auto_remove_timeout)?,
            ),
            (
                config::QuotaEnable::KEY.into(),
                config::QuotaEnable::serialize(&self.quota_enable)?,
            ),
            (
                config::QuotaUserIDs::KEY.into(),
                config::QuotaUserIDs::serialize(&self.quota_user_ids)?,
            ),
            (
                config::QuotaGroupIDs::KEY.into(),
                config::QuotaGroupIDs::serialize(&self.quota_group_ids)?,
            ),
            (
                config::QuotaUpdateInterval::KEY.into(),
                config::QuotaUpdateInterval::serialize(&self.quota_update_interval)?,
            ),
            (
                config::CapPoolMetaLimits::KEY.into(),
                config::CapPoolMetaLimits::serialize(&self.cap_pool_meta_limits)?,
            ),
            (
                config::CapPoolStorageLimits::KEY.into(),
                config::CapPoolStorageLimits::serialize(&self.cap_pool_storage_limits)?,
            ),
        ]
        .into();

        db.execute(|tx| db::config::set(tx, entries)).await
    }
}

#[derive(Clone, Debug, ValueEnum, Deserialize)]
pub enum LogTarget {
    Std,
    Journald,
}

// To be able to parse the log level, we need to make our own enum and convert
// it
#[derive(Clone, Debug, ValueEnum, Deserialize)]
#[repr(usize)]
enum LogLevel {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl From<LogLevel> for LevelFilter {
    fn from(value: LogLevel) -> Self {
        match value {
            LogLevel::Off => LevelFilter::Off,
            LogLevel::Error => LevelFilter::Error,
            LogLevel::Warn => LevelFilter::Warn,
            LogLevel::Info => LevelFilter::Info,
            LogLevel::Debug => LevelFilter::Debug,
            LogLevel::Trace => LevelFilter::Trace,
        }
    }
}
