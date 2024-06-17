//! Functions for getting and setting quota usage information.
use super::*;

/// A pool ID or target ID
pub(crate) enum PoolOrTargetId {
    PoolID(PoolId),
    TargetID(TargetId),
}

/// Calculates IDs that exceed their quota limits for one of the four limit types.
///
/// Since the request message can
/// contain a pool ID or a target ID, both are accepted here as well.
pub(crate) fn exceeded_quota_ids(
    tx: &Transaction,
    pool_or_target_id: PoolOrTargetId,
    id_type: QuotaIdType,
    quota_type: QuotaType,
) -> Result<Vec<QuotaId>> {
    // Quota is calculated per pool, so if a target ID is given, use its assigned pools ID.
    let pool_id = match pool_or_target_id {
        PoolOrTargetId::PoolID(pool_id) => pool_id,
        PoolOrTargetId::TargetID(target_id) => tx.query_row_cached(
            sql!("SELECT pool_id FROM storage_targets WHERE target_id = ?1"),
            [target_id],
            |row| row.get(0),
        )?,
    };

    Ok(tx.query_map_collect(
        sql!(
            "SELECT DISTINCT e.quota_id FROM quota_usage AS e
            INNER JOIN storage_targets AS st USING(target_id)
            LEFT JOIN quota_default_limits AS d USING(id_type, quota_type, pool_id)
            LEFT JOIN quota_limits AS l USING(quota_id, id_type, quota_type, pool_id)
            WHERE e.id_type = ?1 AND e.quota_type = ?2 AND st.pool_id = ?3
            GROUP BY e.quota_id, e.id_type, e.quota_type, st.pool_id
            HAVING SUM(e.value) > COALESCE(l.value, d.value)"
        ),
        params![id_type.sql_str(), quota_type.sql_str(), pool_id],
        |row| row.get(0),
    )?)
}

/// Represents one ID exceeding one of the four quota limits.
///
/// Contains additional information on which limit has been exceeded and on which storage pool.
#[derive(Clone, Debug)]
pub(crate) struct ExceededQuotaEntry {
    pub quota_id: QuotaId,
    pub id_type: QuotaIdType,
    pub quota_type: QuotaType,
    pub pool_id: PoolId,
}

/// Calculates IDs that exceed any of their quota limits.
///
/// Since an ID can exceed more than one of the four limits and also on multiple storage pools, it
/// can be returned more than once (with the respective information stored in [ExceededQuotaEntry]).
pub(crate) fn all_exceeded_quota_ids(tx: &Transaction) -> Result<Vec<ExceededQuotaEntry>> {
    Ok(tx.query_map_collect(
        sql!(
            "SELECT DISTINCT e.quota_id, e.id_type, e.quota_type, st.pool_id
            FROM quota_usage AS e
            INNER JOIN storage_targets AS st USING(target_id)
            LEFT JOIN quota_default_limits AS d USING(id_type, quota_type, pool_id)
            LEFT JOIN quota_limits AS l USING(quota_id, id_type, quota_type, pool_id)
            GROUP BY e.quota_id, e.id_type, e.quota_type, st.pool_id
            HAVING SUM(e.value) > COALESCE(l.value, d.value)"
        ),
        [],
        |row| {
            Ok(ExceededQuotaEntry {
                quota_id: row.get(0)?,
                id_type: QuotaIdType::from_row(row, 1)?,
                quota_type: QuotaType::from_row(row, 2)?,
                pool_id: row.get(3)?,
            })
        },
    )?)
}

/// A quota usage entry containing space and inode usage for a user or group/
#[derive(Clone, Debug)]
pub(crate) struct QuotaData {
    pub quota_id: QuotaId,
    pub id_type: QuotaIdType,
    pub space: u64,
    pub inodes: u64,
}

/// Inserts or updates quota usage entries for a storage target.
pub(crate) fn update(
    tx: &Transaction,
    target_id: TargetId,
    data: impl IntoIterator<Item = QuotaData>,
) -> Result<()> {
    let mut insert_stmt = tx.prepare_cached(sql!(
        "INSERT INTO quota_usage (quota_id, id_type, quota_type, target_id, value)
        VALUES (?1, ?2, ?3 ,?4 ,?5)
        ON CONFLICT (quota_id, id_type, quota_type, target_id) DO
        UPDATE SET value = ?5"
    ))?;

    let mut delete_stmt = tx.prepare_cached(sql!(
        "DELETE FROM quota_usage
        WHERE quota_id = ?1 AND id_type = ?2 AND quota_type = ?3 AND target_id = ?4"
    ))?;

    for d in data {
        match d.space {
            0 => delete_stmt.execute(params![
                d.quota_id,
                d.id_type.sql_str(),
                QuotaType::Space.sql_str(),
                target_id,
            ])?,
            _ => insert_stmt.execute(params![
                d.quota_id,
                d.id_type.sql_str(),
                QuotaType::Space.sql_str(),
                target_id,
                d.space
            ])?,
        };

        match d.inodes {
            0 => delete_stmt.execute(params![
                d.quota_id,
                d.id_type.sql_str(),
                QuotaType::Inodes.sql_str(),
                target_id,
            ])?,
            _ => insert_stmt.execute(params![
                d.quota_id,
                d.id_type.sql_str(),
                QuotaType::Inodes.sql_str(),
                target_id,
                d.inodes
            ])?,
        };
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn upsert_and_get() {
        with_test_data(|tx| {
            update(
                tx,
                1,
                [
                    QuotaData {
                        quota_id: 1000,
                        id_type: QuotaIdType::User,
                        space: 2000,
                        inodes: 0,
                    },
                    QuotaData {
                        quota_id: 1001,
                        id_type: QuotaIdType::User,
                        space: 2000,
                        inodes: 2000,
                    },
                    QuotaData {
                        quota_id: 1002,
                        id_type: QuotaIdType::User,
                        space: 0,
                        inodes: 2000,
                    },
                ],
            )
            .unwrap();

            assert_eq!(
                2,
                exceeded_quota_ids(
                    tx,
                    PoolOrTargetId::PoolID(1),
                    QuotaIdType::User,
                    QuotaType::Space,
                )
                .unwrap()
                .len()
            );

            assert_eq!(
                2,
                exceeded_quota_ids(
                    tx,
                    PoolOrTargetId::PoolID(1),
                    QuotaIdType::User,
                    QuotaType::Inodes,
                )
                .unwrap()
                .len()
            );

            assert_eq!(4, all_exceeded_quota_ids(tx,).unwrap().len());

            update(
                tx,
                1,
                [QuotaData {
                    quota_id: 1001,
                    id_type: QuotaIdType::User,
                    space: 0,
                    inodes: 500,
                }],
            )
            .unwrap();

            assert_eq!(2, all_exceeded_quota_ids(tx,).unwrap().len());
        })
    }
}
