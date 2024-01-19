#[cfg(test)]
use super::test::*;
use crate::error::TypedError;
use crate::types::*;
use anyhow::{anyhow, bail, Result};
use rusqlite::types::Value;
use rusqlite::{params, OptionalExtension, Params, Row, Transaction};
use shared::types::{BuddyGroupID, EntityUID, NodeID, Port, QuotaID, StoragePoolID, TargetID};
use sql_check::sql;
use std::rc::Rc;

pub(crate) mod buddy_group;
pub(crate) mod cap_pool;
pub(crate) mod entity;
pub(crate) mod misc;
pub(crate) mod node;
pub(crate) mod node_nic;
pub(crate) mod quota_default_limit;
pub(crate) mod quota_limit;
pub(crate) mod quota_usage;
pub(crate) mod storage_pool;
pub(crate) mod target;

/// Convienence methods meant for extending [rusqlite::Transaction].
///
/// See the implementation for description.
trait TransactionExt {
    fn execute_cached(&mut self, sql: &str, params: impl Params) -> rusqlite::Result<usize>;
    fn query_row_cached<T, P, F>(&mut self, sql: &str, params: P, f: F) -> rusqlite::Result<T>
    where
        P: Params,
        F: FnOnce(&Row<'_>) -> rusqlite::Result<T>;

    fn query_map_collect<R, C>(
        &mut self,
        sql: &str,
        params: impl Params,
        f: impl FnMut(&Row) -> rusqlite::Result<R>,
    ) -> rusqlite::Result<C>
    where
        C: FromIterator<R>;
}

/// Extends [rusqlite::Transaction] with convenience methods.
impl TransactionExt for Transaction<'_> {
    /// Executes and caches a non-SELECT statement.
    ///
    /// Convenience function for combination of  `.prepare_cached()` and `.execute()`.
    fn execute_cached(&mut self, sql: &str, params: impl Params) -> rusqlite::Result<usize> {
        let mut stmt = self.prepare_cached(sql)?;
        let affected = stmt.execute(params)?;

        Ok(affected)
    }

    /// Executes and caches a SELECT statement returning one row.
    ///
    /// Convenience function for combination of  `.prepare_cached()` and `.query_row()`.
    fn query_row_cached<T, P, F>(&mut self, sql: &str, params: P, f: F) -> rusqlite::Result<T>
    where
        P: Params,
        F: FnOnce(&Row<'_>) -> rusqlite::Result<T>,
    {
        let mut stmt = self.prepare_cached(sql)?;
        stmt.query_row(params, f)
    }

    /// Executes and caches a SELECT statement returning multiple rows, maps them using the
    /// given function and collects them into a collection.
    fn query_map_collect<R, C>(
        &mut self,
        sql: &str,
        params: impl Params,
        f: impl FnMut(&Row) -> rusqlite::Result<R>,
    ) -> rusqlite::Result<C>
    where
        C: FromIterator<R>,
    {
        let mut stmt = self.prepare_cached(sql)?;
        let res = stmt
            .query_map(params, f)?
            .collect::<rusqlite::Result<C>>()?;

        Ok(res)
    }
}

/// Transforms an iterator into a type suitable for passing as a parameter to a rusqlite statement.
///
/// The bound parameter must be accessed using `rarray(?n)` within the statement.
pub(crate) fn rarray_param<T>(iter: impl IntoIterator<Item = T>) -> Rc<Vec<Value>>
where
    Value: From<T>,
{
    Rc::new(iter.into_iter().map(Value::from).collect())
}

/// Checks if the given affected rows count matches one of the allowed entries
///
/// If not, returns an en error
pub(crate) fn check_affected_rows(
    affected: usize,
    allowed: impl IntoIterator<Item = usize>,
) -> Result<()> {
    let res = match allowed.into_iter().any(|e| e == affected) {
        true => Ok(()),
        false => Err(rusqlite::Error::StatementChangedRows(affected)),
    };

    Ok(res?)
}
