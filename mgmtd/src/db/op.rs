use super::error::*;
#[cfg(test)]
use super::test::*;
use entity::EntityUID;
use rusqlite::{params, OptionalExtension, Params, Row, Transaction};
use shared::*;
use std::ops::RangeBounds;

pub mod buddy_group;
pub mod cap_pool;
pub mod config;
pub mod entity;
pub mod misc;
pub mod node;
pub mod node_nic;
pub mod quota_default_limit;
pub mod quota_entry;
pub mod quota_limit;
pub mod storage_pool;
pub mod target;

trait TransactionExt {
    fn is_count_zero(&mut self, sql: &str, params: impl Params) -> rusqlite::Result<bool>;
    fn is_count_zero_cached(&mut self, sql: &str, params: impl Params) -> rusqlite::Result<bool>;
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
impl TransactionExt for Transaction<'_> {
    fn is_count_zero(&mut self, sql: &str, params: impl Params) -> rusqlite::Result<bool> {
        let count: i64 = self.query_row(sql, params, |row| row.get(0))?;
        Ok(count == 0)
    }

    fn is_count_zero_cached(&mut self, sql: &str, params: impl Params) -> rusqlite::Result<bool> {
        let mut stmt = self.prepare_cached(sql)?;
        let count: i64 = stmt.query_row(params, |row| row.get(0))?;
        Ok(count == 0)
    }

    fn execute_cached(&mut self, sql: &str, params: impl Params) -> rusqlite::Result<usize> {
        let mut stmt = self.prepare_cached(sql)?;
        let affected = stmt.execute(params)?;

        Ok(affected)
    }

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

    fn query_row_cached<T, P, F>(&mut self, sql: &str, params: P, f: F) -> rusqlite::Result<T>
    where
        P: Params,
        F: FnOnce(&Row<'_>) -> rusqlite::Result<T>,
    {
        let mut stmt = self.prepare_cached(sql)?;
        stmt.query_row(params, f)
    }
}

fn check_count(count: usize, allowed_range: impl RangeBounds<usize>) -> rusqlite::Result<()> {
    if !allowed_range.contains(&count) {
        Err(rusqlite::Error::StatementChangedRows(count))
    } else {
        Ok(())
    }
}
