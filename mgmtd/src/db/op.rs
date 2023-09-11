use super::error::*;
#[cfg(test)]
use super::test::*;
use rusqlite::{params, OptionalExtension, Params, Row, Transaction};
use shared::*;
use std::ops::RangeBounds;

pub mod buddy_group;
pub mod cap_pool;
pub mod entity;
pub mod misc;
pub mod node;
pub mod node_nic;
pub mod quota_default_limit;
pub mod quota_limit;
pub mod quota_usage;
pub mod storage_pool;
pub mod target;

/// Convienence methods meant for extending [rusqlite::Transaction].
///
/// See the implementation for description.
trait TransactionExt {
    fn execute_cached(&mut self, sql: &str, params: impl Params) -> rusqlite::Result<usize>;
    fn execute_checked(
        &mut self,
        sql: &str,
        params: impl Params,
        allowed_range: impl RangeBounds<usize>,
    ) -> rusqlite::Result<usize>;
    fn execute_checked_cached(
        &mut self,
        sql: &str,
        params: impl Params,
        allowed_range: impl RangeBounds<usize>,
    ) -> rusqlite::Result<usize>;
    fn query_row_cached<T, P, F>(&mut self, sql: &str, params: P, f: F) -> rusqlite::Result<T>
    where
        P: Params,
        F: FnOnce(&Row<'_>) -> rusqlite::Result<T>;
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

    /// Executes and checks a non-SELECT statement.
    ///
    /// After `.execute()` the statement, checks if the affected row count is within the given
    /// range.
    fn execute_checked(
        &mut self,
        sql: &str,
        params: impl Params,
        allowed_range: impl RangeBounds<usize>,
    ) -> rusqlite::Result<usize> {
        let affected = self.execute(sql, params)?;
        check_count(affected, allowed_range)?;

        Ok(affected)
    }

    /// Combines [TransactionExt::execute_cached()] and [TransactionExt::execute_checked()]
    fn execute_checked_cached(
        &mut self,
        sql: &str,
        params: impl Params,
        allowed_range: impl RangeBounds<usize>,
    ) -> rusqlite::Result<usize> {
        let affected = self.execute_cached(sql, params)?;
        check_count(affected, allowed_range)?;

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
}

/// Checks if the given count is within the given range and returns an error if not.
fn check_count(count: usize, allowed_range: impl RangeBounds<usize>) -> rusqlite::Result<()> {
    if !allowed_range.contains(&count) {
        Err(rusqlite::Error::StatementChangedRows(count))
    } else {
        Ok(())
    }
}
