//! A library for defining, obtaining and using dynamic runtime configuration
//! in an asynchronous environment.

use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::fmt::Debug;
use thiserror::Error;

mod cache;
mod cache_input;
mod source;

pub use cache::{from_source, Cache, CacheInput};
pub use source::Source;

#[doc(hidden)]
pub trait GenericConfigValue: Any + Debug + Send + Sync {
    fn as_any(&self) -> &dyn Any;
}

impl<T> GenericConfigValue for T
where
    T: Any + Debug + Send + Sync,
{
    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub type ConfigMap = HashMap<String, String>;
pub type CacheMap = HashMap<String, Box<dyn GenericConfigValue>>;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("(De-)serializing config field {0} failed")]
    Serialization(&'static str, #[source] serde_json::Error),
    #[error("Config key {0} is missing")]
    MissingKey(String),
    #[error("Config key {0} is undefined")]
    UndefinedKey(String),
    #[error("Config key {0} in config definition is not unique (this should never happen)")]
    NonUniqueKey(String),
    #[error("Accessing source failed")]
    SourceError(#[from] BoxedError),
}

pub type BoxedError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[doc(hidden)]
pub trait Field {
    type BelongsTo: Config;
    type Value: Serialize + for<'a> Deserialize<'a> + Clone + Debug + Send + 'static;
    const KEY: &'static str;

    fn default() -> Self::Value;
    fn serialize(value: &Self::Value) -> Result<String, ConfigError> {
        serde_json::to_string(&value).map_err(|err| ConfigError::Serialization(Self::KEY, err))
    }
    fn deserialize(ser: &str) -> Result<Self::Value, ConfigError> {
        serde_json::from_str(ser).map_err(|err| ConfigError::Serialization(Self::KEY, err))
    }
}

pub trait Config {
    const ALL_KEYS: &'static [&'static str];

    fn check_map_completeness(complete_map: &ConfigMap) -> Result<(), ConfigError>;
    fn default_map() -> Result<ConfigMap, ConfigError>;
    fn default_value(key: &str) -> Result<String, ConfigError>;
    fn deserialize_to_any(
        key: &str,
        value: &str,
    ) -> Result<Box<dyn GenericConfigValue>, ConfigError>;
}

#[macro_export]
macro_rules! define_config {
    (struct $struct_name:ident, $($key:ident: $type:ty = $default_value:expr,)+) => {
        use $crate::Field;

        $(
            pub struct $key {}
            impl $crate::Field for $key {
                type BelongsTo = $struct_name;
                type Value = $type;
                const KEY: &'static str = stringify!($key);

                fn default() -> Self::Value {
                    $default_value
                }
            }
        )+

        #[derive(Clone, Debug)]
        pub struct $struct_name;
        impl $crate::Config for $struct_name {
            const ALL_KEYS: &'static[&'static str] = &[
                $(
                    $key::KEY,
                )+
            ];

            fn default_value(key: &str) -> Result<String, $crate::ConfigError> {
                match key {
                    $(
                        $key::KEY => $key::serialize(&$key::default()),
                    )+
                    key => Err($crate::ConfigError::UndefinedKey(key.to_string()))
                }
            }

            fn default_map() -> Result<$crate::ConfigMap, $crate::ConfigError> {
                let mut map = std::collections::HashMap::new();

                $(
                    map.insert($key::KEY.to_string(), $key::serialize(&$key::default())?);
                )+

                Ok(map)
            }

            fn deserialize_to_any(key: &str, value: &str) -> Result<Box<dyn $crate::GenericConfigValue>, $crate::ConfigError> {
                match key {
                    $(
                        $key::KEY => {
                            let value = $key::deserialize(value)?;
                            Ok((Box::new(value)))
                        }
                    )+
                    key => {return Err($crate::ConfigError::UndefinedKey(key.to_string()));}
                }
            }

            fn check_map_completeness(complete_map: &$crate::ConfigMap) -> Result<(), $crate::ConfigError> {
                for (key, _) in complete_map {
                    match Self::ALL_KEYS.iter().filter(|k| k == &key).count() {
                        0 => return Err($crate::ConfigError::UndefinedKey(key.to_string())),
                        1 => {},
                        _ => return Err($crate::ConfigError::NonUniqueKey(key.to_string()))
                    }
                }

                for key in Self::ALL_KEYS.iter() {
                    match complete_map.iter().filter(|e| e.0 == key).count() {
                        0 => return Err($crate::ConfigError::MissingKey(key.to_string())),
                        1 => {},
                        _ => unreachable!("HashMap doesn't allow duplicated keys")
                    }
                }

                Ok(())
            }
        }
    };
}
