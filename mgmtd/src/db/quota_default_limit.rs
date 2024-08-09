//! Functions for getting and setting quota default limits.
//!
//! Quota default limits are quota limits applying to a storage pool and all users / groups that
//! don't have a explicit limit assigned.
use super::*;
use rusqlite::OptionalExtension;

/// A set of quota default limits.
#[derive(Debug, Clone, Default)]
pub(crate) struct DefaultLimits {
    pub user_space_limit: Option<u64>,
    pub user_inodes_limit: Option<u64>,
    pub group_space_limit: Option<u64>,
    pub group_inodes_limit: Option<u64>,
}

/// Retrieves the default limits for the given storage pool ID.
pub(crate) fn get_with_pool_id(tx: &Transaction, pool_id: PoolId) -> Result<DefaultLimits> {
    let _ = resolve_num_id(tx, EntityType::Pool, NodeType::Storage, pool_id.into())?;

    let limits = tx
        .query_row_cached(
            sql!(
                "SELECT DISTINCT us.value, ui.value, gs.value, gi.value
                FROM quota_default_limits AS l
                LEFT JOIN quota_default_limits AS us
                    ON us.pool_id = l.pool_id AND us.quota_type = 1 AND us.id_type = 1
                LEFT JOIN quota_default_limits AS ui
                    ON ui.pool_id = l.pool_id AND ui.quota_type = 2 AND ui.id_type = 1
                LEFT JOIN quota_default_limits AS gs
                    ON gs.pool_id = l.pool_id AND gs.quota_type = 1 AND gs.id_type = 2
                LEFT JOIN quota_default_limits AS gi
                    ON gi.pool_id = l.pool_id AND gi.quota_type = 2 AND gi.id_type = 2
                WHERE l.pool_id = ?1"
            ),
            params![pool_id],
            |row| {
                Ok(DefaultLimits {
                    user_space_limit: row.get(0)?,
                    user_inodes_limit: row.get(1)?,
                    group_space_limit: row.get(2)?,
                    group_inodes_limit: row.get(3)?,
                })
            },
        )
        .optional()?;

    Ok(limits.unwrap_or_default())
}

/// Inserts or updates one default limit for the given storage pool ID.
///
/// Affects one of the four limits ((user, group) x (space, inode)).
pub(crate) fn upsert(
    tx: &Transaction,
    pool_id: PoolId,
    id_type: QuotaIdType,
    quota_type: QuotaType,
    value: u64,
) -> Result<()> {
    tx.execute_cached(
        sql!(
            "INSERT INTO quota_default_limits (id_type, quota_type, pool_id, value)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT (id_type, quota_type, pool_id) DO
            UPDATE SET value = ?4
            WHERE id_type = ?1 AND quota_type = ?2 AND pool_id = ?3"
        ),
        params![
            id_type.sql_variant(),
            quota_type.sql_variant(),
            pool_id,
            value
        ],
    )?;

    Ok(())
}

/// Deletes one default limit for the given storage pool ID.
///
/// Affects one of the four limits ((user, group) x (space, inode)).
pub(crate) fn delete(
    tx: &Transaction,
    pool_id: PoolId,
    id_type: QuotaIdType,
    quota_type: QuotaType,
) -> Result<()> {
    let _ = resolve_num_id(tx, EntityType::Pool, NodeType::Storage, pool_id.into())?;

    tx.execute_cached(
        sql!(
            "DELETE FROM quota_default_limits
            WHERE id_type = ?1 AND quota_type = ?2 AND pool_id = ?3"
        ),
        params![id_type.sql_variant(), quota_type.sql_variant(), pool_id],
    )?;

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn set_get() {
        with_test_data(|tx| {
            let defaults = super::get_with_pool_id(tx, 1).unwrap();

            assert_eq!(Some(1000), defaults.user_space_limit);
            assert_eq!(Some(1000), defaults.user_inodes_limit);
            assert_eq!(Some(1000), defaults.group_space_limit);
            assert_eq!(Some(1000), defaults.group_inodes_limit);

            let defaults = super::get_with_pool_id(tx, 2).unwrap();

            assert_eq!(None, defaults.user_space_limit);
            assert_eq!(None, defaults.user_inodes_limit);
            assert_eq!(None, defaults.group_space_limit);
            assert_eq!(None, defaults.group_inodes_limit);

            super::delete(tx, 1, QuotaIdType::User, QuotaType::Space).unwrap();
            super::upsert(tx, 1, QuotaIdType::User, QuotaType::Inode, 2000).unwrap();

            let defaults = super::get_with_pool_id(tx, 1).unwrap();

            assert_eq!(None, defaults.user_space_limit);
            assert_eq!(Some(2000), defaults.user_inodes_limit);
            assert_eq!(Some(1000), defaults.group_space_limit);
            assert_eq!(Some(1000), defaults.group_inodes_limit);
        })
    }
}
