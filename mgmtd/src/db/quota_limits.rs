use super::*;
use itertools::Itertools;
use rusqlite::{OptionalExtension, ToSql};
use std::ops::RangeInclusive;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct SpaceAndInodeLimits {
    pub quota_id: QuotaID,
    pub space: Option<Space>,
    pub inodes: Option<Inodes>,
}

pub(crate) fn with_quota_id(
    tx: &mut Transaction,
    quota_id: QuotaID,
    pool_id: StoragePoolID,
    id_type: QuotaIDType,
) -> Result<SpaceAndInodeLimits> {
    let mut stmt = tx.prepare_cached(
        r#"
        SELECT space_value, inodes_value FROM quota_limits_combined_v
        WHERE quota_id = ?1 AND pool_id == ?2 AND id_type = ?3
        "#,
    )?;

    let (space, inodes) = stmt
        .query_row(params![quota_id, pool_id, id_type], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .optional()?
        .unwrap_or_default();

    Ok(SpaceAndInodeLimits {
        quota_id,
        space,
        inodes,
    })
}

pub(crate) fn with_quota_id_range(
    tx: &mut Transaction,
    quota_id_range: RangeInclusive<QuotaID>,
    pool_id: StoragePoolID,
    id_type: QuotaIDType,
) -> Result<Vec<SpaceAndInodeLimits>> {
    fetch(
        tx,
        r#"
        SELECT space_value, inodes_value, quota_id FROM quota_limits_combined_v
        WHERE quota_id >= ?1 AND quota_id <= ?2 AND pool_id == ?3 AND id_type = ?4
        "#,
        params![
            quota_id_range.start(),
            quota_id_range.end(),
            pool_id,
            id_type
        ],
    )
}

pub(crate) fn with_quota_id_list(
    tx: &mut Transaction,
    quota_ids: impl IntoIterator<Item = QuotaID>,
    pool_id: StoragePoolID,
    id_type: QuotaIDType,
) -> Result<Vec<SpaceAndInodeLimits>> {
    fetch(
        tx,
        &format!(
            r#"
        SELECT quota_id, space_value, inodes_value FROM quota_limits_combined_v
        WHERE pool_id == ?1 AND id_type = ?2
        AND quota_id IN ({})
        "#,
            quota_ids.into_iter().join(",")
        ),
        params![pool_id, id_type],
    )
}

pub(crate) fn all(
    tx: &mut Transaction,
    pool_id: StoragePoolID,
    id_type: QuotaIDType,
) -> Result<Vec<SpaceAndInodeLimits>> {
    fetch(
        tx,
        r#"
        SELECT quota_id, space_value, inodes_value FROM quota_limits_combined_v
        WHERE pool_id == ?1 AND id_type = ?2
        "#,
        params![pool_id, id_type],
    )
}

fn fetch(
    tx: &mut Transaction,
    stmt: &str,
    params: &[&dyn ToSql],
) -> Result<Vec<SpaceAndInodeLimits>> {
    let mut stmt = tx.prepare_cached(stmt)?;

    let res = stmt
        .query_map(params, |row| {
            Ok(SpaceAndInodeLimits {
                quota_id: row.get(0)?,
                space: row.get(1)?,
                inodes: row.get(2)?,
            })
        })?
        .try_collect()?;

    Ok(res)
}

pub(crate) fn update(
    tx: &mut Transaction,
    iter: impl IntoIterator<Item = (QuotaIDType, StoragePoolID, SpaceAndInodeLimits)>,
) -> Result<()> {
    let mut insert_stmt = tx.prepare_cached(
        r#"
        INSERT INTO quota_limits (quota_id, id_type, quota_type, pool_id, value)
        VALUES(?1, ?2, ?3 ,?4 ,?5)
        ON CONFLICT (quota_id, id_type, quota_type, pool_id) DO
        UPDATE SET value = ?5
        "#,
    )?;

    let mut delete_stmt = tx.prepare_cached(
        r#"
        DELETE FROM quota_limits
        WHERE quota_id = ?1 AND id_type = ?2 AND quota_type = ?3 AND pool_id == ?4
        "#,
    )?;

    for l in iter {
        if let Some(space) = l.2.space {
            insert_stmt.execute(params![l.2.quota_id, l.0, QuotaType::Space, l.1, space])?;
        } else {
            delete_stmt.execute(params![l.2.quota_id, l.0, QuotaType::Space, l.1])?;
        }

        if let Some(inodes) = l.2.inodes {
            insert_stmt.execute(params![l.2.quota_id, l.0, QuotaType::Inodes, l.1, inodes])?;
        } else {
            delete_stmt.execute(params![l.2.quota_id, l.0, QuotaType::Inodes, l.1])?;
        }
    }

    Ok(())
}
