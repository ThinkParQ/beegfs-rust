//! Program wide config definition and tools for reading and parsing

use crate::cap_pool::{CapPoolDynamicLimits, CapPoolLimits};
use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use log::LevelFilter;
use serde::{Deserialize, Deserializer};
use shared::parser::{duration, integer_range};
use shared::types::{Port, QuotaId};
use std::fmt::Debug;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::time::Duration;

/// Generates `Config` to be filled by the functions below and exported to be used by the program.
///
/// Note that the attributes attached to each item have a fixed order, e.g. doc comments come first,
/// then #[arg()] and finally #[serde()].
///
/// The short help should show the values default value (except for bools). These have to be put
/// at the end of the first doc comment line manually (see existing flags).
macro_rules! generate_structs {
    (
        $(
            $(#[doc = $doc:literal])*
            $(#[arg($($clap_arg:tt)+)])*
            $(#[serde($($serde_arg:tt)+)])*
            $var:ident: $typ:ty = $default:expr,
        )*
    ) => {
        /// The user configuration
        #[derive(Debug)]
        pub struct Config {
            $(
                $(#[doc = $doc])*
                pub $var: $typ,
            )*
        }

        impl Default for Config {
            fn default() -> Self {
                Self {
                    $(
                        $var: $default,
                    )*
                }
            }
        }

        impl Config {
            fn update_from_optional(&mut self, args: OptionalConfig) {
                $(
                    if let Some(v) = args.$var {
                        self.$var = v.into();
                    }
                )*
            }
        }

        // The below text is used as general help text

        /// The BeeGFS management service
        ///
        /// To set up a new system, use `--init`. To upgrade an existing database, use `--upgrade`.
        ///
        /// To specify a config file to load configuration from, use `--config-file`. Command line
        /// parameters overwrite config file parameters. If there is a config file in the default
        /// location, it is loaded automatically. Note that there are some parameters that can only
        /// be set by using a config file (mainly quota and capacity poool related).
        #[derive(Debug, Default, Parser, Deserialize)]
        #[command(
            version = version_str(),
            rename_all = "kebab-case",
        )]
        #[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
        struct OptionalConfig {
            $(
                $(#[doc = $doc])*
                $(#[arg($($clap_arg)+)])*
                $(#[serde($($serde_arg)+)])*
                $var: Option<$typ>,
            )*
        }

    };
}

// Deserialization / parser helpers

fn deserialize_optional_u32_range<'de, D: Deserializer<'de>>(
    de: D,
) -> Result<Option<Option<RangeInclusive<u32>>>, D::Error> {
    Ok(Some(Some(integer_range::deserialize(de)?)))
}

fn deserialize_duration<'de, D: Deserializer<'de>>(de: D) -> Result<Option<Duration>, D::Error> {
    Ok(Some(duration::deserialize(de)?))
}

generate_structs! {
    /// Creates and initializes a new database, then exits.
    ///
    /// Uses `--db-file` for the files location or the default if not given. Denies overwriting an
    /// existing database file.
    #[arg(long)]
    #[arg(num_args = 0..=1, default_missing_value = "true")]
    #[serde(skip)]
    init: bool = false,

    /// Upgrades an outdated management database to the current version, then exits.
    ///
    /// Automatically creates a backup of the existing database file in the same directory.
    #[arg(long)]
    #[arg(num_args = 0..=1, default_missing_value = "true")]
    #[serde(skip)]
    upgrade: bool = false,

    /// Imports a BeeGFS v7 installation from the provided directory into a new database.
    ///
    /// The database file must not exist yet. Before importing a production BeeGFS, ensure that
    /// all targets are in GOOD state, all clients are unmounted and the whole system has been
    /// shutdown. After importing the data, verify its correctness by only starting the management
    /// and checking the existing nodes, targets, buddy groups, storage pools and quota settings.
    ///
    /// The database file will only be created if the whole import succeeds.
    #[arg(long)]
    #[arg(num_args = 1)]
    #[arg(value_name = "PATH")]
    #[serde(skip)]
    import_from_v7: Option<PathBuf> = None,

    /// Loads additional configuration from the given file. [default = "/etc/beegfs/beegfs-mgmtd.toml"]
    ///
    /// Config file settings overwrite the default settings and command line settings
    /// overwrite config file settings.
    #[arg(long)]
    #[arg(value_name = "PATH")]
    #[serde(skip)]
    config_file: PathBuf = "/etc/beegfs/beegfs-mgmtd.toml".into(),

    /// Managements database file location. [default: /var/lib/beegfs/mgmtd.sqlite]
    #[arg(long)]
    #[arg(value_name = "PATH")]
    db_file: PathBuf = "/var/lib/beegfs/mgmtd.sqlite".into(),

    /// The log target to use. [default: journald]
    #[arg(long)]
    #[arg(value_name = "IDENT")]
    log_target: LogTarget = LogTarget::Journald,

    /// The log level to use. [default: warn]
    ///
    /// Sets the maximum level to log. When logging to std, the logging behavior can be fine
    /// controlled by setting the RUST_LOG environment variable. This overwrites this setting. See
    /// the env_logger documentation for more details:
    /// <https://docs.rs/env_logger/latest/env_logger/#enabling-logging>
    #[arg(long)]
    #[arg(value_name = "IDENT")]
    log_level: LogLevel = LogLevel::Warn,

    // Connection

    /// Sets the BeeMsg / "classic" port (TCP and UDP) to listen on. [default: 8008]
    #[arg(long)]
    #[arg(value_name = "PORT")]
    beemsg_port: Port = 8008,

    /// Sets the gRPC port to listen on. [default: 8010]
    #[arg(long)]
    #[arg(value_name = "PORT")]
    grpc_port: Port = 8010,

    /// Disables TLS for gRPC communication.
    #[arg(long)]
    #[arg(num_args = 0..=1, default_missing_value = "true")]
    tls_disable: bool = false,

    /// The PEM encoded .X509 certificate file that provides the identity of the gRPC server.
    /// [default: /etc/beegfs/cert.pem]
    #[arg(long)]
    #[arg(value_name = "PATH")]
    tls_cert_file: PathBuf = "/etc/beegfs/cert.pem".into(),

    /// The private key file belonging to the above certificate. [default: /etc/beegfs/key.pem]
    #[arg(long)]
    #[arg(value_name = "PATH")]
    tls_key_file: PathBuf = "/etc/beegfs/key.pem".into(),

    /// Restricts network interfaces reported to other nodes for incoming BeeMsg communication.
    ///
    /// Accepts a comma separated list of interface names. They are reported in the given order. If
    /// not given, all suitable interfaces can be used.
    #[arg(long)]
    #[arg(value_name = "NAMES")]
    #[arg(value_delimiter = ',')]
    interfaces: Vec<String> = vec![],

    /// Maximum number of outgoing BeeMsg connections per node. [default: 12]
    #[arg(long)]
    #[arg(value_name = "LIMIT")]
    connection_limit: usize = 12,

    /// Disables requiring authentication (BeeMsg and gRPC).
    #[arg(long)]
    #[arg(num_args = 0..=1, default_missing_value = "true")]
    auth_disable: bool = false,

    /// The authentication file location [default: /etc/beegfs/conn.auth]
    #[arg(long)]
    #[arg(value_name = "PATH")]
    auth_file: PathBuf = "/etc/beegfs/conn.auth".into(),

    /// General

    /// Disables registration of new nodes and targets (clients excluded).
    #[arg(long)]
    #[arg(num_args = 0..=1, default_missing_value = "true")]
    registration_disable: bool = false,

    /// Defines after which time without contact a node/target is considered offline. [default: 180s]
    ///
    /// IMPORTANT: This setting must be the same on all nodes in the system, especially when using
    /// mirroring.
    #[arg(long, )]
    #[arg(value_name = "DURATION")]
    #[arg(value_parser = duration::parse)]
    #[serde(deserialize_with = "deserialize_duration")]
    node_offline_timeout: Duration = Duration::from_secs(180),

    /// Defines after which time without contact a client is considered gone and will be removed.
    /// [default: 30m]
    #[arg(long)]
    #[arg(value_name = "DURATION")]
    #[arg(value_parser = duration::parse)]
    #[serde(deserialize_with = "deserialize_duration")]
    client_auto_remove_timeout: Duration = Duration::from_secs(30 * 60),

    /// Disables loading the license library.
    ///
    /// This disables all enterprise features.
    #[arg(long)]
    #[arg(num_args = 0..=1, default_missing_value = "true")]
    license_disable: bool = false,

    /// The BeeGFS license certificate file. [default: /etc/beegfs/license.pem]
    #[arg(long)]
    #[arg(value_name = "PATH")]
    license_cert_file: PathBuf = "/etc/beegfs/license.pem".into(),

    /// The BeeGFS license library file. [default: /opt/beegfs/lib/libbeegfs_license.so]
    #[arg(long)]
    #[arg(value_name = "PATH")]
    license_lib_file: PathBuf = "/opt/beegfs/lib/libbeegfs_license.so".into(),


    /// Maximum number of blocking worker threads. [default: 128]
    ///
    /// These are started on demand and kept running for some time in idle state before being
    /// dropped again. Currently, they are only used for parallel database operations. Each thread
    /// uses its own sqlite connection, meaning an extra open file. Therefore, this settings also
    /// limits the maximum number of open sqlite files of the process.
    ///
    /// This setting only affects systems with high read operation load and should usually be left
    /// alone.
    #[arg(long)]
    #[arg(value_name = "LIMIT")]
    max_blocking_threads: usize = 128,

    // Quota

    /// Enables quota data collection and checks.
    ///
    /// Allows querying the state and setting limits (which do nothing without enforcement being
    /// enabled). Causes higher system load.
    #[arg(long)]
    #[arg(num_args = 0..=1, default_missing_value = "true")]
    quota_enable: bool = false,

    /// Enables quota enforcement.
    ///
    /// Exceeded IDs are calculated and pushed to the servers on a regular basis. Requires
    /// quota_enable = true. Causes higher system load.
    #[arg(long)]
    #[arg(num_args = 0..=1, default_missing_value = "true")]
    quota_enforce: bool = false,

    /// Update interval of quota information. [default: 30s]
    ///
    /// Defines how often the management pulls the quota information from all storage nodes,
    /// calculates the IDs that exceed the limits and reports them back to the server nodes.
    #[arg(long)]
    #[arg(value_name = "DURATION")]
    #[arg(value_parser = duration::parse)]
    #[serde(deserialize_with = "deserialize_duration")]
    quota_update_interval: Duration = Duration::from_secs(30),

    /// Defines the minimum id of the existing system users to be quota checked and enforced.
    ///
    /// Note that this uses the users from the local machine the management is running on.
    #[arg(long)]
    #[arg(num_args = 1)] // Overwrite the automatic `num_args = 0..=1`
    #[arg(value_name = "ID")]
    quota_user_system_ids_min: Option<QuotaId> = None,
    /// Loads the user ids to be quota queried and enforced from a file.
    ///
    /// Ids must be numeric only and separated by any whitespace.
    #[arg(long)]
    #[arg(num_args = 1)]
    #[arg(value_name = "PATH")]
    quota_user_ids_file: Option<PathBuf> = None,
    /// Defines a range of user ids to be quota queried and enforced.
    #[arg(long)]
    #[arg(num_args = 1)]
    #[arg(value_name = "RANGE")]
    // Despite below being clap parsed into an `Option<Option<RangeInclusive>>`,
    // value_parser still needs to output the "raw" value as `Result<RangeInclusive>`
    #[arg(value_parser = integer_range::parse::<u32>)]
    #[serde(deserialize_with = "deserialize_optional_u32_range")]
    quota_user_ids_range: Option<RangeInclusive<u32>> = None,

    /// Defines the minimum id of the existing system groups to be quota checked and enforced.
    ///
    /// Note that this uses the groups from the local machine the management is running on.
    #[arg(long)]
    #[arg(num_args = 1)]
    #[arg(value_name = "ID")]
    quota_group_system_ids_min: Option<QuotaId> = None,
    /// Loads the group ids to be quota queried and enforced from a file.
    ///
    /// Ids must be numeric only and separated by any whitespace.
    #[arg(long)]
    #[arg(num_args = 1)]
    #[arg(value_name = "PATH")]
    quota_group_ids_file: Option<PathBuf> = None,
    /// Defines a range of group ids to be quota queried and enforced.
    #[arg(long)]
    #[arg(num_args = 1)]
    #[arg(value_name = "RANGE")]
    #[arg(value_parser = integer_range::parse::<u32>)]
    #[serde(deserialize_with = "deserialize_optional_u32_range")]
    quota_group_ids_range: Option<RangeInclusive<u32>> = None,

    // Capacity pools

    /// Sets the limits / boundaries of the meta capacity pools.
    #[arg(skip)]
    cap_pool_meta_limits: CapPoolLimits = CapPoolLimits {
        inodes_low: 10 * 1000 * 1000,
        inodes_emergency: 1000 * 1000,
        space_low: 10 * 1024 * 1024 * 1024,
        space_emergency: 3 * 1024 * 1024 * 1024,
    },
    /// Sets the limits / boundaries of the dynamic meta capacity pools and the thresholds that determine
    /// which limits shall be used.
    #[arg(skip)]
    cap_pool_dynamic_meta_limits: Option<CapPoolDynamicLimits> = None,

    /// Sets the limits / boundaries of the meta capacity pools.
    #[arg(skip)]
    cap_pool_storage_limits: CapPoolLimits = CapPoolLimits {
        inodes_low: 10 * 1000 * 1000,
        inodes_emergency: 1000 * 1000,
        space_low: 512 * 1024 * 1024 * 1024,
        space_emergency: 10 * 1024 * 1024 * 1024,
    },
    /// Sets the limits / boundaries of the dynamic meta capacity pools and the thresholds that determine
    /// which limits shall be used.
    #[arg(skip)]
    cap_pool_dynamic_storage_limits: Option<CapPoolDynamicLimits> = None,

    // Daemonization

    /// Daemonize the process by forking.
    ///
    /// Not very user friendly due to the logging, thus hidden. Normal users should use systemd.
    #[arg(long)]
    #[arg(hide = true)]
    #[arg(num_args = 0..=1, default_missing_value = "true")]
    daemonize: bool = false,

    /// The pid file location for the daemonized process.
    #[arg(long)]
    #[arg(hide = true)]
    #[arg(value_name = "PATH")]
    daemonize_pid_file: PathBuf = "/run/beegfs/mgmtd.pid".into(),
}

impl Config {
    pub fn check_validity(&self) -> Result<()> {
        if self.quota_enforce && !self.quota_enable {
            bail!("Quota enforcement requires quota being enabled");
        }

        self.cap_pool_meta_limits
            .check()
            .context("Capacity pool meta limits")?;

        self.cap_pool_storage_limits
            .check()
            .context("Capacity pool storage limits")?;

        if let Some(ref l) = self.cap_pool_dynamic_meta_limits {
            l.check().context("Capacity pool dynamic meta limits")?;

            if l.space_low < self.cap_pool_meta_limits.space_low
                || l.inodes_low < self.cap_pool_meta_limits.inodes_low
                || l.space_emergency < self.cap_pool_meta_limits.space_emergency
                || l.inodes_emergency < self.cap_pool_meta_limits.inodes_emergency
            {
                bail!(
                    "At least one capacity pool dynamic meta limit is lower than the default limit"
                );
            }
        }

        if let Some(ref l) = self.cap_pool_dynamic_storage_limits {
            l.check().context("Capacity pool dynamic storage limits")?;

            if l.space_low < self.cap_pool_storage_limits.space_low
                || l.inodes_low < self.cap_pool_storage_limits.inodes_low
                || l.space_emergency < self.cap_pool_storage_limits.space_emergency
                || l.inodes_emergency < self.cap_pool_storage_limits.inodes_emergency
            {
                bail!("At least one capacity pool dynamic storage limit is lower than the default limit");
            }
        }

        Ok(())
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
    let command_config = OptionalConfig::parse();

    let config_file = command_config
        .config_file
        .as_ref()
        .unwrap_or(&config.config_file);

    match std::fs::read_to_string(config_file) {
        Ok(ref toml_config) => {
            let file_config: OptionalConfig =
                toml::from_str(toml_config).with_context(|| "Could not parse config file")?;

            info_log.push(format!("Loaded config file from {:?}", config_file));
            config.update_from_optional(file_config);
        }
        Err(err) => {
            if config_file != &config.config_file {
                return Err(err)
                    .with_context(|| format!("Could not open config file at {:?}", config_file));
            }

            info_log.push("No config file found at default location, ignoring".to_string());
        }
    }

    config.update_from_optional(command_config);
    config.check_validity().context("Invalid config")?;
    Ok((config, info_log))
}

/// Constructs a version str from the `VERSION` environment variable at compile time
const fn version_str() -> &'static str {
    match option_env!("VERSION") {
        Some(version) => version,
        None => "undefined",
    }
}

// Custom types for user input

/// Defines where log messages shall be sent to
#[derive(Clone, Debug, ValueEnum, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LogTarget {
    Std,
    Journald,
}

/// Defines the log level
#[derive(Clone, Debug, ValueEnum, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LogLevel {
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
