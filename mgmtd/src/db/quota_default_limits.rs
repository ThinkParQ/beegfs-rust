use super::*;
use rusqlite::OptionalExtension;

#[derive(Debug, Clone, Default)]
pub(crate) struct DefaultLimits {
    pub user_space_limit: Option<Space>,
    pub user_inode_limit: Option<Inodes>,
    pub group_space_limit: Option<Space>,
    pub group_inode_limit: Option<Inodes>,
}

pub(crate) fn with_pool_id(tx: &mut Transaction, pool_id: StoragePoolID) -> Result<DefaultLimits> {
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
                user_inode_limit: row.get(1)?,
                group_space_limit: row.get(2)?,
                group_inode_limit: row.get(3)?,
            })
        })
        .optional()?;

    Ok(limits.unwrap_or_default())
}

pub(crate) fn update(
    tx: &mut Transaction,
    pool_id: StoragePoolID,
    id_type: QuotaIDType,
    space: Option<Space>,
    inodes: Option<Inodes>,
) -> Result<()> {
    let mut stmt = tx.prepare_cached(
        r#"
        INSERT INTO quota_default_limits (id_type, quota_type, pool_id, value)
        VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT (id_type, quota_type, pool_id) DO
        UPDATE SET value = ?4
        WHERE id_type = ?1 AND quota_type = ?2 AND pool_id = ?3
        "#,
    )?;

    if let Some(space) = space {
        stmt.execute(params![id_type, QuotaType::Space, pool_id, space])?;
    }

    if let Some(inodes) = inodes {
        stmt.execute(params![id_type, QuotaType::Inodes, pool_id, inodes])?;
    }

    Ok(())
}
