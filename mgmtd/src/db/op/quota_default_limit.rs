use super::*;
use rusqlite::OptionalExtension;

#[derive(Debug, Clone, Default)]
pub struct DefaultLimits {
    pub user_space_limit: Option<u64>,
    pub user_inodes_limit: Option<u64>,
    pub group_space_limit: Option<u64>,
    pub group_inodes_limit: Option<u64>,
}

pub fn with_pool_id(tx: &mut Transaction, pool_id: StoragePoolID) -> DbResult<DefaultLimits> {
    storage_pool::get_uid(tx, pool_id)?
        .ok_or_else(|| DbError::value_not_found("storage pool ID", pool_id))?;

    let mut stmt = tx.prepare_cached(
        r#"
        SELECT user_space_value, user_inodes_value, group_space_value, group_inodes_value
        FROM quota_default_limits_combined_v
        WHERE pool_id = ?1
        "#,
    )?;

    let limits = stmt
        .query_row(params![pool_id], |row| {
            Ok(DefaultLimits {
                user_space_limit: row.get(0)?,
                user_inodes_limit: row.get(1)?,
                group_space_limit: row.get(2)?,
                group_inodes_limit: row.get(3)?,
            })
        })
        .optional()?;

    Ok(limits.unwrap_or_default())
}

pub fn update(
    tx: &mut Transaction,
    pool_id: StoragePoolID,
    id_type: QuotaIDType,
    quota_type: QuotaType,
    value: u64,
) -> DbResult<()> {
    storage_pool::get_uid(tx, pool_id)?
        .ok_or_else(|| DbError::value_not_found("storage pool ID", pool_id))?;

    let mut stmt = tx.prepare_cached(
        r#"
        INSERT INTO quota_default_limits (id_type, quota_type, pool_id, value)
        VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT (id_type, quota_type, pool_id) DO
        UPDATE SET value = ?4
        WHERE id_type = ?1 AND quota_type = ?2 AND pool_id = ?3
        "#,
    )?;

    stmt.execute(params![id_type, quota_type, pool_id, value])?;

    Ok(())
}

pub fn delete(
    tx: &mut Transaction,
    pool_id: StoragePoolID,
    id_type: QuotaIDType,
    quota_type: QuotaType,
) -> DbResult<()> {
    storage_pool::get_uid(tx, pool_id)?
        .ok_or_else(|| DbError::value_not_found("storage pool ID", pool_id))?;

    let mut stmt = tx.prepare_cached(
        r#"
        DELETE FROM quota_default_limits
        WHERE id_type = ?1 AND quota_type = ?2 AND pool_id = ?3
        "#,
    )?;

    stmt.execute(params![id_type, quota_type, pool_id])?;

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn set_get() {
        with_test_data(|tx| {
            let defaults = super::with_pool_id(tx, 1.into()).unwrap();

            assert_eq!(Some(1000), defaults.user_space_limit);
            assert_eq!(Some(1000), defaults.user_inodes_limit);
            assert_eq!(Some(1000), defaults.group_space_limit);
            assert_eq!(Some(1000), defaults.group_inodes_limit);

            let defaults = super::with_pool_id(tx, 2.into()).unwrap();

            assert_eq!(None, defaults.user_space_limit);
            assert_eq!(None, defaults.user_inodes_limit);
            assert_eq!(None, defaults.group_space_limit);
            assert_eq!(None, defaults.group_inodes_limit);

            super::delete(tx, 1.into(), QuotaIDType::User, QuotaType::Space).unwrap();
            super::update(tx, 1.into(), QuotaIDType::User, QuotaType::Inodes, 2000).unwrap();

            let defaults = super::with_pool_id(tx, 1.into()).unwrap();

            assert_eq!(None, defaults.user_space_limit);
            assert_eq!(Some(2000), defaults.user_inodes_limit);
            assert_eq!(Some(1000), defaults.group_space_limit);
            assert_eq!(Some(1000), defaults.group_inodes_limit);
        })
    }
}
