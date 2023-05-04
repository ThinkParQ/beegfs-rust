use super::*;

pub(crate) enum PoolOrTargetID {
    PoolID(StoragePoolID),
    TargetID(TargetID),
}

pub(crate) fn exceeded_quota_ids(
    tx: &mut Transaction,
    pool_or_target_id: PoolOrTargetID,
    id_type: QuotaIDType,
    quota_type: QuotaType,
) -> Result<Vec<QuotaID>> {
    let pool_id = match pool_or_target_id {
        PoolOrTargetID::PoolID(pool_id) => pool_id,
        PoolOrTargetID::TargetID(target_id) => {
            let mut stmt = tx.prepare_cached(
                r#"
                SELECT pool_id FROM storage_targets WHERE target_id = ?1
                "#,
            )?;

            stmt.query_row([target_id], |row| row.get(0))?
        }
    };

    let mut stmt = tx.prepare_cached(
        r#"
        SELECT DISTINCT quota_id
        FROM exceeded_quota_entries_v
        WHERE id_type = ?1 AND quota_type = ?2 AND pool_id = ?3
        "#,
    )?;

    let ids = stmt
        .query_map(params![id_type, quota_type, pool_id], |row| row.get(0))?
        .try_collect()?;

    Ok(ids)
}

#[derive(Clone, Debug)]
pub(crate) struct ExceededQuotaEntry {
    pub quota_id: QuotaID,
    pub id_type: QuotaIDType,
    pub quota_type: QuotaType,
    pub pool_id: StoragePoolID,
}

pub(crate) fn exceeded_quota_entries(tx: &mut Transaction) -> Result<Vec<ExceededQuotaEntry>> {
    let mut stmt = tx.prepare_cached(
        r#"
        SELECT quota_id, id_type, quota_type, pool_id
        FROM exceeded_quota_entries_v
        "#,
    )?;

    let res = stmt
        .query_map([], |row| {
            Ok(ExceededQuotaEntry {
                quota_id: row.get(0)?,
                id_type: row.get(1)?,
                quota_type: row.get(2)?,
                pool_id: row.get(3)?,
            })
        })?
        .try_collect()?;

    Ok(res)
}

#[derive(Clone, Debug)]
pub(crate) struct QuotaData {
    pub quota_id: QuotaID,
    pub id_type: QuotaIDType,
    pub space: Space,
    pub inodes: Inodes,
}

pub(crate) fn upsert(
    tx: &mut Transaction,
    target_id: TargetID,
    data: impl IntoIterator<Item = QuotaData>,
) -> Result<()> {
    let mut insert_stmt = tx.prepare_cached(
        r#"
        INSERT INTO quota_entries (quota_id, id_type, quota_type, target_id, value)
        VALUES (?1, ?2, ?3 ,?4 ,?5)
        ON CONFLICT (quota_id, id_type, quota_type, target_id) DO
        UPDATE SET value = ?5
        "#,
    )?;

    let mut delete_stmt = tx.prepare_cached(
        r#"
        DELETE FROM quota_entries
        WHERE quota_id = ?1 AND id_type = ?2 AND quota_type = ?3 AND target_id = ?4
        "#,
    )?;

    for d in data {
        match d.space {
            Space::ZERO => {
                delete_stmt.execute(params![d.quota_id, d.id_type, QuotaType::Space, target_id,])?
            }
            _ => insert_stmt.execute(params![
                d.quota_id,
                d.id_type,
                QuotaType::Space,
                target_id,
                d.space
            ])?,
        };

        match d.inodes {
            Inodes::ZERO => {
                delete_stmt
                    .execute(params![d.quota_id, d.id_type, QuotaType::Inodes, target_id,])?
            }
            _ => delete_stmt.execute(params![
                d.quota_id,
                d.id_type,
                QuotaType::Inodes,
                target_id,
                d.space
            ])?,
        };
    }

    Ok(())
}
