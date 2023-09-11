//! Program wide config definition and tools for reading and parsing

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use log::LevelFilter;
use serde::Deserialize;
use shared::parser::integer_with_time_unit;
use shared::{CapPoolDynamicLimits, CapPoolLimits, Port, QuotaID};
use std::fmt::Debug;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Contains the program configuration
///
/// Filled by command line flags and config file and provides
/// access to them for various parts of the program. Meant to be read-only after initialization.
///
/// Parameters added here must be set to be updated by either config file or command line or both
/// below.
#[derive(Debug)]
pub struct Config {
    pub init: bool,
    pub port: Port,
    pub interfaces: Vec<String>,
    pub connection_limit: usize,
    pub db_file: PathBuf,
    pub auth_file: PathBuf,
    pub auth_enable: bool,
    pub log_target: LogTarget,
    pub log_level: LevelFilter,
    pub registration_enable: bool,
    pub node_offline_timeout: Duration,
    pub client_auto_remove_timeout: Duration,

    // Quota
    pub quota_enable: bool,
    pub quota_update_interval: Duration,

    pub quota_user_system_ids_min: Option<QuotaID>,
    pub quota_user_ids_file: Option<PathBuf>,
    pub quota_user_ids_range: Option<RangeInclusive<u32>>,
    pub quota_group_system_ids_min: Option<QuotaID>,
    pub quota_group_ids_file: Option<PathBuf>,
    pub quota_group_ids_range: Option<RangeInclusive<u32>>,

    // Capacity pools
    pub cap_pool_meta_limits: CapPoolLimits,
    pub cap_pool_storage_limits: CapPoolLimits,

    // Dynamic capacity pools
    pub cap_pool_dynamic_meta_limits: Option<CapPoolDynamicLimits>,
    pub cap_pool_dynamic_storage_limits: Option<CapPoolDynamicLimits>,
}

/// Sets the default values for the configuration.
///
/// Used when the parameter is provided neither by command line nor by config file.
impl Default for Config {
    fn default() -> Self {
        Self {
            init: false,
            port: 8008.into(),
            interfaces: vec![],
            connection_limit: 12,
            db_file: "/var/lib/beegfs/mgmtd.sqlite".into(),
            auth_file: "/etc/beegfs/mgmtd.auth".into(),
            auth_enable: true,
            log_target: LogTarget::Journald,
            log_level: LevelFilter::Warn,

            registration_enable: true,
            node_offline_timeout: Duration::from_secs(180),
            client_auto_remove_timeout: Duration::from_secs(30 * 60),

            // Quota
            quota_enable: false,
            quota_update_interval: Duration::from_secs(30),

            quota_user_system_ids_min: None,
            quota_user_ids_file: None,
            quota_user_ids_range: None,
            quota_group_system_ids_min: None,
            quota_group_ids_file: None,
            quota_group_ids_range: None,

            // Capacity pools
            cap_pool_meta_limits: CapPoolLimits {
                inodes_low: 10 * 1000 * 1000,
                inodes_emergency: 1000 * 1000,
                space_low: 10 * 1024 * 1024 * 1024,
                space_emergency: 3 * 1024 * 1024 * 1024,
            },
            cap_pool_storage_limits: CapPoolLimits {
                inodes_low: 10 * 1000 * 1000,
                inodes_emergency: 1000 * 1000,
                space_low: 512 * 1024 * 1024 * 1024,
                space_emergency: 10 * 1024 * 1024 * 1024,
            },

            // Dynamic capacity pools
            cap_pool_dynamic_meta_limits: None,
            cap_pool_dynamic_storage_limits: None,
        }
    }
}

// Defines the Clap command line interface. Doc comment for the struct defines title and main help
// text.
//
/// BeeGFS mgmtd Rust prototype
#[derive(Debug, Default, Parser)]
#[command(
    author,
    version,
    rename_all = "kebab-case",
    hide_possible_values = false
)]
struct CommandLineArgs {
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
    /// Maximum number of outgoing connections per node [default: 12]
    #[arg(long)]
    connection_limit: Option<usize>,
    /// Sqlite database file location [default: /var/lib/beegfs/mgmtd.sqlite]
    #[arg(long)]
    db_file: Option<PathBuf>,
    /// Enable authentication [default: true]
    #[arg(long)]
    auth_enable: Option<bool>,
    /// Authentication file location [default: /etc/beegfs/mgmtd.auth]
    #[arg(long)]
    auth_file: Option<PathBuf>,
    /// Log target [default: journald]
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
    /// <https://docs.rs/env_logger/latest/env_logger/#enabling-logging>
    #[arg(long)]
    log_level: Option<LogLevel>,

    // CLI only args - we do not parse them from file and also do not update them
    /// Initialialize a new installation, then quit
    #[arg(long)]
    init: bool,
    /// Config file location
    #[arg(long, default_value = "/etc/beegfs/mgmtd.toml")]
    config_file: PathBuf,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ConfigFileArgs {
    port: Option<Port>,
    interfaces: Option<Vec<String>>,
    connection_limit: Option<usize>,
    db_file: Option<PathBuf>,
    auth_enable: Option<bool>,
    auth_file: Option<PathBuf>,
    log_target: Option<LogTarget>,
    log_level: Option<LogLevel>,

    registration_enable: Option<bool>,
    #[serde(with = "integer_with_time_unit::optional")]
    node_offline_timeout: Option<Duration>,
    #[serde(with = "integer_with_time_unit::optional")]
    client_auto_remove_timeout: Option<Duration>,
    quota_enable: Option<bool>,
    #[serde(with = "integer_with_time_unit::optional")]
    quota_update_interval: Option<Duration>,
    quota_user_system_ids_min: Option<QuotaID>,
    quota_user_ids_file: Option<PathBuf>,
    quota_user_ids_range_start: Option<u32>,
    quota_user_ids_range_end: Option<u32>,
    quota_group_system_ids_min: Option<QuotaID>,
    quota_group_ids_file: Option<PathBuf>,
    quota_group_ids_range_start: Option<u32>,
    quota_group_ids_range_end: Option<u32>,
    cap_pool_meta_limits: Option<CapPoolLimits>,
    cap_pool_storage_limits: Option<CapPoolLimits>,
    cap_pool_dynamic_meta_limits: Option<CapPoolDynamicLimits>,
    cap_pool_dynamic_storage_limits: Option<CapPoolDynamicLimits>,
}

impl Config {
    /// Update parameters from the command line parameter struct
    ///
    /// Non-Option parameters in this struct are only updates if they are `Some` in
    /// [[CommandLineArgs]], otherwise they were not given and shall stay as they are.
    fn update_from_command_line_args(&mut self, args: CommandLineArgs) {
        self.init = args.init;
        if let Some(v) = args.port {
            self.port = v;
        }
        if let Some(v) = args.interfaces {
            self.interfaces = v;
        }
        if let Some(v) = args.connection_limit {
            self.connection_limit = v;
        }
        if let Some(v) = args.db_file {
            self.db_file = v;
        }
        if let Some(v) = args.auth_file {
            self.auth_file = v;
        }
        if let Some(v) = args.auth_enable {
            self.auth_enable = v;
        }
        if let Some(v) = args.log_target {
            self.log_target = v;
        }
        if let Some(v) = args.log_level {
            self.log_level = v.into();
        }
    }

    /// Update parameters from the config file parameter struct
    ///
    /// Non-Option parameters in this struct are only updates if they are `Some` in
    /// [[ConfigFileArgs]], otherwise they were not given and shall stay as they are.
    fn update_from_config_file_args(&mut self, args: ConfigFileArgs) {
        if let Some(v) = args.port {
            self.port = v;
        }
        if let Some(v) = args.interfaces {
            self.interfaces = v;
        }
        if let Some(v) = args.connection_limit {
            self.connection_limit = v;
        }
        if let Some(v) = args.db_file {
            self.db_file = v;
        }
        if let Some(v) = args.auth_enable {
            self.auth_enable = v;
        }
        if let Some(v) = args.auth_file {
            self.auth_file = v;
        }
        if let Some(v) = args.log_target {
            self.log_target = v;
        }
        if let Some(v) = args.log_level {
            self.log_level = v.into();
        }

        if let Some(v) = args.registration_enable {
            self.registration_enable = v;
        }
        if let Some(v) = args.node_offline_timeout {
            self.node_offline_timeout = v;
        }
        if let Some(v) = args.client_auto_remove_timeout {
            self.client_auto_remove_timeout = v;
        }
        if let Some(v) = args.quota_enable {
            self.quota_enable = v;
        }
        if let Some(v) = args.quota_update_interval {
            self.quota_update_interval = v;
        }

        // This (and more below) is actually an Option, so we just replace it
        //
        // TODO this does not allow to UNSET this option from command line when given in the config
        // file. Maybe we should change that
        self.quota_user_system_ids_min = args.quota_user_system_ids_min;
        self.quota_user_ids_file = args.quota_user_ids_file;
        if let (Some(s), Some(e)) = (
            args.quota_user_ids_range_start,
            args.quota_user_ids_range_end,
        ) {
            self.quota_user_ids_range = Some(s..=e);
        }

        self.quota_group_system_ids_min = args.quota_group_system_ids_min;
        self.quota_group_ids_file = args.quota_group_ids_file;
        if let (Some(s), Some(e)) = (
            args.quota_group_ids_range_start,
            args.quota_group_ids_range_end,
        ) {
            self.quota_group_ids_range = Some(s..=e);
        }

        if let Some(v) = args.cap_pool_meta_limits {
            self.cap_pool_meta_limits = v;
        }
        if let Some(v) = args.cap_pool_storage_limits {
            self.cap_pool_storage_limits = v;
        }

        self.cap_pool_dynamic_meta_limits = args.cap_pool_dynamic_meta_limits;
        self.cap_pool_dynamic_storage_limits = args.cap_pool_dynamic_storage_limits;
    }
}

/// Loads and parses configuration.
///
/// The following order is used (latter ones overwrite former ones, having higher precedence):
/// 1. Default config.
/// 2. Parameters from config file if either present at default location or specified on the command
///    line
/// 3. Parameters from given on the command line
///
/// # Return value
///
/// Returns a tuple consisting of the [[Config]] object and a vec of strings containing log
/// messages. Since the log system might not  be initialized yet, this allows the caller to log
/// the messages later.
pub fn load_and_parse() -> Result<(Config, Vec<String>)> {
    let mut info_log = vec![];
    let mut config = Config::default();
    let command_line_args = CommandLineArgs::parse();

    match std::fs::read_to_string(&command_line_args.config_file) {
        Ok(ref toml_config) => {
            let config_file_args: ConfigFileArgs =
                toml::from_str(toml_config).with_context(|| "Could not parse config file")?;

            info_log.push(format!(
                "Loaded config file from {:?}",
                command_line_args.config_file
            ));

            config.update_from_config_file_args(config_file_args);
        }
        Err(err) => {
            if command_line_args.config_file != Path::new("/etc/beegfs/mgmtd.toml") {
                return Err(err).with_context(|| {
                    format!(
                        "Could not open config file at {:?}",
                        command_line_args.config_file
                    )
                });
            }

            info_log.push("No config file found at default location, ignoring".to_string());
        }
    }

    config.update_from_command_line_args(command_line_args);

    Ok((config, info_log))
}

/// Custom types for user input

/// Defines where log messages shall be sent to
#[derive(Clone, Debug, ValueEnum, Deserialize)]
pub enum LogTarget {
    Std,
    Journald,
}

/// Defines the log level
#[derive(Clone, Debug, ValueEnum, Deserialize)]
enum LogLevel {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// Conversion of user given log level into type used by log crate
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
