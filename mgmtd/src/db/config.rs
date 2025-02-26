use anyhow::{Result, bail};
use rusqlite::{Transaction, params};
use sqlite::{TransactionExt, check_affected_rows};
use sqlite_check::sql;
use std::str::FromStr;

/// The list of config entries.
///
/// Note: Remove the `allow()`` below when adding other variants than `Fs*`
#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Config {
    FsUuid,
    FsInitDateSecs,
    /// Note: Remove the `allow(unused)` when actually using this entry (currently it's only used
    /// for the test as it's the only entry not marked as immutable)
    #[allow(unused)]
    FsName,
    CounterLastClientID,
}

// Config entries that should not be changed after initially set. Note that this only controls the
// functions below, the database entries could still be changed by manual query
const IMMUTABLE: &[Config] = &[Config::FsUuid, Config::FsInitDateSecs];

impl Config {
    /// The string representation of the config key as it is written to the db
    fn str(&self) -> &'static str {
        match self {
            Config::FsUuid => "fs_uuid",
            Config::FsInitDateSecs => "fs_init_date_secs",
            Config::FsName => "fs_name",
            Config::CounterLastClientID => "counter_last_client_id",
        }
    }
}

/// Set a config entry. Automatically inserts it if it doesn't exist yet. Config entries in the
/// IMMUTABLE list can not be updated.
pub(crate) fn set<V>(tx: &Transaction, key: Config, value: V) -> Result<()>
where
    V: ToString,
{
    let key_str = key.str();
    let value_str = value.to_string();

    if IMMUTABLE.contains(&key) {
        let value = get::<String>(tx, key)?;

        if let Some(value) = value {
            bail!("{key:?} is marked as immutable and already set to {value}");
        }
    }

    let affected = tx.execute_cached(
        sql!("REPLACE INTO config (key, value) VALUES (?1, ?2)"),
        params![key_str, value_str],
    )?;

    check_affected_rows(affected, [1])?;

    Ok(())
}

/// Get a config entry if it exists
pub(crate) fn get<V>(tx: &Transaction, key: Config) -> Result<Option<V>>
where
    V: FromStr,
    anyhow::Error: From<V::Err>,
{
    let key_str = key.str();

    // Doing query manually instead of query_row to avoid an extra String allocation b
    let mut stmt = tx.prepare_cached(sql!("SELECT value FROM config WHERE key = ?1"))?;
    let mut res = stmt.query([key_str])?;
    let row = res.next()?;

    let Some(row) = row else {
        return Ok(None);
    };

    let value_str = row.get_ref(0)?.as_str()?;
    let value = V::from_str(value_str)?;

    Ok(Some(value))
}

/// Delete a config entry. Config entries in the IMMUTABLE list cannot be deleted.
#[allow(unused)]
pub(crate) fn delete(tx: &Transaction, key: Config) -> Result<()> {
    if IMMUTABLE.contains(&key) {
        bail!("{key:?} is marked as immutable and cannot be deleted");
    }

    let key_str = key.str();
    let affected = tx.execute_cached(sql!("DELETE FROM config WHERE key = ?1"), [key_str])?;
    check_affected_rows(affected, [1])
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::db::test::with_test_data;

    #[test]
    fn set_get_delete() {
        with_test_data(|tx| {
            assert!(IMMUTABLE.contains(&Config::FsUuid));

            set(tx, Config::FsUuid, 1000).unwrap();

            // Change of immutable FileSystemId should be denied
            set(tx, Config::FsUuid, 2000).unwrap_err();

            assert_eq!(Option::<String>::None, get(tx, Config::FsName).unwrap());

            set(tx, Config::FsName, "lustre").unwrap();
            set(tx, Config::FsName, "beegfs").unwrap();

            assert_eq!(Some(1000), get(tx, Config::FsUuid).unwrap());
            assert_eq!(Some("beegfs".to_string()), get(tx, Config::FsName).unwrap());

            delete(tx, Config::FsName).unwrap();

            // Deletion of immutable FileSystemId should be denied
            delete(tx, Config::FsUuid).unwrap_err();

            // Check it's gone
            assert_eq!(Option::<String>::None, get(tx, Config::FsName).unwrap());
        });
    }
}
