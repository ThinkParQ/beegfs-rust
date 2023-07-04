use crate::db::{self, Connection};
use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use log::LevelFilter;
use serde::{Deserialize, Serialize};
use shared::parser::integer_with_time_unit;
use shared::{CapPoolDynamicLimits, CapPoolLimits, Port, QuotaID};
use std::fmt::Debug;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, RwLockReadGuard};
use std::time::Duration;

// DYNAMIC CONFIG

pub(crate) trait Field {
    type Value: Serialize + for<'a> Deserialize<'a> + Clone + Debug + Send + Sync + 'static;
    const KEY: &'static str;

    fn default() -> Self::Value;
}

macro_rules! define_config {
    { $($key:ident: $type:ty = $default_value:expr,)+ } => {
        $(
            #[allow(non_camel_case_types)]
            pub(crate) struct $key {}

            impl Field for $key {
                type Value = $type;
                const KEY: &'static str = stringify!($key);

                fn default() -> Self::Value {
                    $default_value
                }
            }
        )+

        #[derive(Debug)]
        pub(crate) struct DynamicConfig {
            $(
                pub $key: $type,
            )+
        }

        impl Default for DynamicConfig {
            fn default() -> Self {
                Self {
                    $(
                        $key: $default_value,
                    )+
                }
            }
        }

        impl DynamicConfig {
            pub(crate) fn set_by_json_str(&mut self, key: &str, json_value: &str) -> serde_json::Result<()> {
                match key {
                    $(
                        stringify!($key) => self.$key = serde_json::from_str(json_value)?,
                    )+
                    _ => {}
                }

                Ok(())
            }
        }
    }
}

define_config! {
    registration_enable: bool = true,
    node_offline_timeout: Duration = Duration::from_secs(180),
    client_auto_remove_timeout: Duration = Duration::from_secs(30 * 60),

    // Quota
    quota_enable: bool = false,
    quota_update_interval: Duration = Duration::from_secs(30),

    quota_user_system_ids_min: Option<QuotaID> = None,
    quota_user_ids_file: Option<PathBuf> = None,
    quota_user_ids_range: Option<RangeInclusive<u32>> = None,
    quota_group_system_ids_min: Option<QuotaID> = None,
    quota_group_ids_file: Option<PathBuf> = None,
    quota_group_ids_range: Option<RangeInclusive<u32>> = None,

    // Capacity pools
    cap_pool_meta_limits: CapPoolLimits = CapPoolLimits {
        inodes_low: 10 * 1000 * 1000,
        inodes_emergency: 1000 * 1000,
        space_low: 10 * 1024 * 1024 * 1024,
        space_emergency: 3 * 1024 * 1024 * 1024
    },
    cap_pool_storage_limits: CapPoolLimits = CapPoolLimits {
        inodes_low: 10 * 1000 * 1000,
        inodes_emergency: 1000 * 1000,
        space_low: 512 * 1024 * 1024 * 1024,
        space_emergency: 10 * 1024 * 1024 * 1024
    },

    // Dynamic capacity pools
    cap_pool_dynamic_meta_limits: Option<CapPoolDynamicLimits> = None,
    cap_pool_dynamic_storage_limits: Option<CapPoolDynamicLimits> = None,
}

#[derive(Debug, Clone)]
pub(crate) struct ConfigCache {
    inner: Arc<RwLock<DynamicConfig>>,
    db: Connection,
}

impl ConfigCache {
    pub(crate) async fn from_db(db: Connection) -> Result<Self> {
        let config = db.execute(db::config::get_all).await?;

        Ok(Self {
            inner: Arc::new(RwLock::new(config)),
            db,
        })
    }

    #[allow(unused)]
    pub(crate) async fn set<T: Field>(&self, value: T::Value) -> Result<()> {
        self.inner
            .write()
            .expect("Lock is poisoned")
            .set_by_json_str(T::KEY, &serde_json::to_string(&value)?)?;

        self.db
            .execute(move |tx| db::config::set_one::<T>(tx, &value))
            .await?;

        Ok(())
    }

    pub(crate) fn get(&self) -> RwLockReadGuard<DynamicConfig> {
        self.inner.read().expect("Lock is poisoned")
    }
}

// STATIC CONFIG

#[derive(Debug)]
pub struct StaticConfig {
    pub init: bool,
    pub port: Port,
    pub interfaces: Vec<String>,
    pub db_file: PathBuf,
    pub auth_file: PathBuf,
    pub auth_enable: bool,
    pub log_target: LogTarget,
    pub log_level: LevelFilter,
}

impl Default for StaticConfig {
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

// PARSING AND LOADING

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

    fn into_config(self) -> StaticConfig {
        let mut config = StaticConfig {
            init: self.init,
            ..StaticConfig::default()
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

#[derive(Debug, Serialize, Deserialize)]
pub struct DynamicConfigArgs {
    registration_enable: bool,
    #[serde(with = "integer_with_time_unit")]
    node_offline_timeout: Duration,
    #[serde(with = "integer_with_time_unit")]
    client_auto_remove_timeout: Duration,
    quota_enable: bool,
    #[serde(with = "integer_with_time_unit")]
    quota_update_interval: Duration,
    quota_user_system_ids_min: Option<QuotaID>,
    quota_user_ids_file: Option<PathBuf>,
    quota_user_ids_range_start: Option<u32>,
    quota_user_ids_range_end: Option<u32>,
    quota_group_system_ids_min: Option<QuotaID>,
    quota_group_ids_file: Option<PathBuf>,
    quota_group_ids_range_start: Option<u32>,
    quota_group_ids_range_end: Option<u32>,
    cap_pool_meta_limits: CapPoolLimits,
    cap_pool_storage_limits: CapPoolLimits,
    cap_pool_dynamic_meta_limits: Option<CapPoolDynamicLimits>,
    cap_pool_dynamic_storage_limits: Option<CapPoolDynamicLimits>,
}

impl DynamicConfigArgs {
    pub async fn apply_to_db(self, db: &db::Connection) -> anyhow::Result<()> {
        db.execute(move |tx| {
            use db::config::*;

            set_one::<registration_enable>(tx, &self.registration_enable)?;
            set_one::<node_offline_timeout>(tx, &self.node_offline_timeout)?;
            set_one::<client_auto_remove_timeout>(tx, &self.client_auto_remove_timeout)?;
            set_one::<quota_enable>(tx, &self.quota_enable)?;
            set_one::<quota_update_interval>(tx, &self.quota_update_interval)?;
            set_one::<quota_user_system_ids_min>(tx, &self.quota_user_system_ids_min)?;
            set_one::<quota_user_ids_file>(tx, &self.quota_user_ids_file)?;
            set_one::<quota_user_ids_range>(
                tx,
                &self
                    .quota_user_ids_range_start
                    .map(|start| start..=self.quota_user_ids_range_end.unwrap_or(start)),
            )?;
            set_one::<quota_group_system_ids_min>(tx, &self.quota_group_system_ids_min)?;
            set_one::<quota_group_ids_file>(tx, &self.quota_group_ids_file)?;
            set_one::<quota_group_ids_range>(
                tx,
                &self
                    .quota_group_ids_range_start
                    .map(|start| start..=self.quota_group_ids_range_end.unwrap_or(start)),
            )?;
            set_one::<cap_pool_meta_limits>(tx, &self.cap_pool_meta_limits)?;
            set_one::<cap_pool_storage_limits>(tx, &self.cap_pool_storage_limits)?;
            set_one::<cap_pool_dynamic_meta_limits>(tx, &self.cap_pool_dynamic_meta_limits)?;
            set_one::<cap_pool_dynamic_storage_limits>(tx, &self.cap_pool_dynamic_storage_limits)?;

            Ok(())
        })
        .await?;

        Ok(())
    }
}

pub fn load_and_parse() -> Result<(StaticConfig, Option<DynamicConfigArgs>, Vec<String>)> {
    let mut info_log = vec![];
    let mut args = ConfigArgs::parse();

    if let Some(ref file) = args.config_file {
        match std::fs::read_to_string(file) {
            Ok(ref toml_config) => {
                let file_args: ConfigArgs =
                    toml::from_str(toml_config).with_context(|| "Couldn't parse config file")?;

                info_log.push(format!("Loaded node configuration from {file:?}"));

                args.fill_from(file_args);
            }
            Err(err) => {
                if file != Path::new("/etc/beegfs/mgmtd.toml") {
                    return Err(err)
                        .with_context(|| format!("Could not open config file at {file:?}"));
                }

                info_log.push("No config file found at default location, ignoring".to_string());
            }
        }
    }

    let tmp_runtime_config = if let Some(ref file) = args.runtime_config_file {
        match std::fs::read_to_string(file) {
            Ok(ref toml_config) => {
                let config_values: DynamicConfigArgs = toml::from_str(toml_config)
                    .with_context(|| "Couldn't parse runtime config file")?;

                info_log.push(format!("Loaded runtime configuration from {file:?}"));

                Some(config_values)
            }
            Err(err) => {
                if file != Path::new("/etc/beegfs/mgmtd-runtime.toml") {
                    return Err(err).with_context(|| {
                        format!("Could not open runtime config file at {file:?}")
                    });
                }

                info_log
                    .push("No runtime config file found at default location, ignoring".to_string());

                None
            }
        }
    } else {
        None
    };

    Ok((args.into_config(), tmp_runtime_config, info_log))
}

#[derive(Clone, Debug, ValueEnum, Deserialize)]
pub enum LogTarget {
    Std,
    Journald,
}

// To be able to parse the log level, we need to make our own enum and convert
// it
#[derive(Clone, Debug, ValueEnum, Deserialize)]
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
