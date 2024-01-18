//! Functions for getting and setting quota usage information.
use super::*;

/// A pool ID or target ID
pub(crate) enum PoolOrTargetID {
    PoolID(StoragePoolID),
    TargetID(TargetID),
}

/// Calculates IDs that exceed their quota limits for one of the four limit types.
///
/// Since the request message can
/// contain a pool ID or a target ID, both are accepted here as well.
pub(crate) fn exceeded_quota_ids(
    tx: &mut Transaction,
    pool_or_target_id: PoolOrTargetID,
    id_type: QuotaIDType,
    quota_type: QuotaType,
) -> Result<Vec<QuotaID>> {
    // Quota is calculated per pool, so if a target ID is given, use its assigned pools ID.
    let pool_id = match pool_or_target_id {
        PoolOrTargetID::PoolID(pool_id) => pool_id,
        PoolOrTargetID::TargetID(target_id) => tx.query_row_cached(
            sql!("SELECT pool_id FROM storage_targets WHERE target_id = ?1"),
            [target_id],
            |row| row.get(0),
        )?,
    };

    Ok(tx.query_map_collect(
        sql!(
            "SELECT DISTINCT quota_id
            FROM exceeded_quota_v
            WHERE id_type = ?1 AND quota_type = ?2 AND pool_id = ?3"
        ),
        params![id_type, quota_type, pool_id],
        |row| row.get(0),
    )?)
}

/// Represents one ID exceeding one of the four quota limits.
///
/// Contains additional information on which limit has been exceeded and on which storage pool.
#[derive(Clone, Debug)]
pub(crate) struct ExceededQuotaEntry {
    pub quota_id: QuotaID,
    pub id_type: QuotaIDType,
    pub quota_type: QuotaType,
    pub pool_id: StoragePoolID,
}

/// Calculates IDs that exceed any of their quota limits.
///
/// Since an ID can exceed more than one of the four limits and also on multiple storage pools, it
/// can be returned more than once (with the respective information stored in [ExceededQuotaEntry]).
pub(crate) fn all_exceeded_quota_ids(tx: &mut Transaction) -> Result<Vec<ExceededQuotaEntry>> {
    Ok(tx.query_map_collect(
        sql!("SELECT quota_id, id_type, quota_type, pool_id FROM exceeded_quota_v"),
        [],
        |row| {
            Ok(ExceededQuotaEntry {
                quota_id: row.get(0)?,
                id_type: row.get(1)?,
                quota_type: row.get(2)?,
                pool_id: row.get(3)?,
            })
        },
    )?)
}

/// A quota usage entry containing space and inode usage for a user or group/
#[derive(Clone, Debug)]
pub(crate) struct QuotaData {
    pub quota_id: QuotaID,
    pub id_type: QuotaIDType,
    pub space: u64,
    pub inodes: u64,
}

/// Inserts or updates quota usage entries for a storage target.
pub(crate) fn upsert(
    tx: &mut Transaction,
    target_id: TargetID,
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
            0 => {
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
            0 => {
                delete_stmt
                    .execute(params![d.quota_id, d.id_type, QuotaType::Inodes, target_id,])?
            }
            _ => insert_stmt.execute(params![
                d.quota_id,
                d.id_type,
                QuotaType::Inodes,
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
            upsert(
                tx,
                1,
                [
                    QuotaData {
                        quota_id: 1000,
                        id_type: QuotaIDType::User,
                        space: 2000,
                        inodes: 0,
                    },
                    QuotaData {
                        quota_id: 1001,
                        id_type: QuotaIDType::User,
                        space: 2000,
                        inodes: 2000,
                    },
                    QuotaData {
                        quota_id: 1002,
                        id_type: QuotaIDType::User,
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
                    PoolOrTargetID::PoolID(1),
                    QuotaIDType::User,
                    QuotaType::Space,
                )
                .unwrap()
                .len()
            );

            assert_eq!(
                2,
                exceeded_quota_ids(
                    tx,
                    PoolOrTargetID::PoolID(1),
                    QuotaIDType::User,
                    QuotaType::Inodes,
                )
                .unwrap()
                .len()
            );

            assert_eq!(4, all_exceeded_quota_ids(tx,).unwrap().len());

            upsert(
                tx,
                1,
                [QuotaData {
                    quota_id: 1001,
                    id_type: QuotaIDType::User,
                    space: 0,
                    inodes: 500,
                }],
            )
            .unwrap();

            assert_eq!(2, all_exceeded_quota_ids(tx,).unwrap().len());
        })
    }
}
