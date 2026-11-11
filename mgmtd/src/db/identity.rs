use super::config::{self, Config};
use anyhow::Result;
use rusqlite::Transaction;

pub(crate) fn version(tx: &Transaction) -> Result<u64> {
    Ok(config::get(tx, Config::IdentitiesVersion)?.unwrap_or(0))
}

pub(crate) fn bump_version(tx: &Transaction) -> Result<()> {
    config::set(tx, Config::IdentitiesVersion, version(tx)? + 1)
}
