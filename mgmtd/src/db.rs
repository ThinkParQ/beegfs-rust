use crate::ensure_rows_modified;
use anyhow::{bail, Result};
use rusqlite::{params, Transaction};
use shared::*;
use thiserror::Error;

pub(crate) mod buddy_groups;
pub(crate) mod cap_pools;
pub(crate) mod config;
pub(crate) mod misc;
pub(crate) mod node_nics;
pub(crate) mod nodes;
pub(crate) mod quota_default_limits;
pub(crate) mod quota_entries;
pub(crate) mod quota_limits;
pub(crate) mod sqlite;
pub(crate) mod storage_pools;
pub(crate) mod targets;

#[cfg(test)]
mod tests;

pub(crate) use sqlite::Handle;

#[derive(Debug, Error)]
#[error("{0} doesn't exist")]
pub(crate) struct NonexistingKey(pub(crate) String);

#[macro_export]
/// Tests the amount of affected rows after an UPDATE or DELETE, bail if 0
macro_rules! ensure_rows_modified {
    ($affected:ident, $key:expr) => {
        match $affected {
            0 => {
                anyhow::bail!($crate::db::NonexistingKey(format!("{:?}", $key)));
            }
            _ => {
                // Ok
            }
        }
    };

    ($affected:ident, $($key:expr),+) => {
        match $affected {
            0 => {
                let key_tuple = (
                    $(
                        $key,
                    )+
                );
                ::anyhow::bail!($crate::db::NonexistingKey(format!("{key_tuple:?}")));
            }
            _ => {
                // Ok
            }
        }
    };
}
