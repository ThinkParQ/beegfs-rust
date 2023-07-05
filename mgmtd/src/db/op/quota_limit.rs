use super::*;
use itertools::Itertools;
use rusqlite::ToSql;
use std::ops::RangeInclusive;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct SpaceAndInodeLimits {
    pub quota_id: QuotaID,
    pub space: Option<u64>,
    pub inodes: Option<u64>,
}

pub fn with_quota_id_range(
    tx: &mut Transaction,
    quota_id_range: RangeInclusive<QuotaID>,
    pool_id: StoragePoolID,
    id_type: QuotaIDType,
) -> DbResult<Vec<SpaceAndInodeLimits>> {
    fetch(
        tx,
        r#"
        SELECT quota_id, space_value, inodes_value FROM quota_limits_combined_v
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

pub fn with_quota_id_list(
    tx: &mut Transaction,
    quota_ids: impl IntoIterator<Item = QuotaID>,
    pool_id: StoragePoolID,
    id_type: QuotaIDType,
) -> DbResult<Vec<SpaceAndInodeLimits>> {
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

pub fn all(
    tx: &mut Transaction,
    pool_id: StoragePoolID,
    id_type: QuotaIDType,
) -> DbResult<Vec<SpaceAndInodeLimits>> {
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
) -> DbResult<Vec<SpaceAndInodeLimits>> {
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

pub fn update(
    tx: &mut Transaction,
    iter: impl IntoIterator<Item = (QuotaIDType, StoragePoolID, SpaceAndInodeLimits)>,
) -> DbResult<()> {
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn set_get() {
        with_test_data(|tx| {
            assert_eq!(0, all(tx, 1.into(), QuotaIDType::User).unwrap().len());

            update(
                tx,
                [
                    (
                        QuotaIDType::User,
                        1.into(),
                        SpaceAndInodeLimits {
                            quota_id: 1000.into(),
                            space: Some(2000),
                            inodes: None,
                        },
                    ),
                    (
                        QuotaIDType::User,
                        1.into(),
                        SpaceAndInodeLimits {
                            quota_id: 1001.into(),
                            space: None,
                            inodes: Some(2000),
                        },
                    ),
                    (
                        QuotaIDType::User,
                        1.into(),
                        SpaceAndInodeLimits {
                            quota_id: 1002.into(),
                            space: Some(2000),
                            inodes: Some(2000),
                        },
                    ),
                ],
            )
            .unwrap();

            assert_eq!(3, all(tx, 1.into(), QuotaIDType::User).unwrap().len());
            assert_eq!(
                2,
                with_quota_id_range(tx, 900.into()..=1001.into(), 1.into(), QuotaIDType::User)
                    .unwrap()
                    .len()
            );
            assert_eq!(
                2,
                with_quota_id_list(
                    tx,
                    [900.into(), 1000.into(), 1002.into()],
                    1.into(),
                    QuotaIDType::User
                )
                .unwrap()
                .len()
            );

            update(
                tx,
                [
                    (
                        QuotaIDType::User,
                        1.into(),
                        SpaceAndInodeLimits {
                            quota_id: 1000.into(),
                            space: Some(2000),
                            inodes: Some(2000),
                        },
                    ),
                    (
                        QuotaIDType::User,
                        1.into(),
                        SpaceAndInodeLimits {
                            quota_id: 1001.into(),
                            space: None,
                            inodes: None,
                        },
                    ),
                ],
            )
            .unwrap();

            assert_eq!(2, all(tx, 1.into(), QuotaIDType::User).unwrap().len());
        })
    }

    const BENCH_QUOTA_ID_NUM: u32 = 1000;

    #[bench]
    fn bench_quota_limits_read(b: &mut Bencher) {
        let mut conn = setup_on_disk_db();
        let mut counter = 0;

        transaction(&mut conn, |tx| {
            update(
                tx,
                (1..=BENCH_QUOTA_ID_NUM).map(|e| {
                    (
                        QuotaIDType::User,
                        1.into(),
                        SpaceAndInodeLimits {
                            quota_id: e.into(),
                            space: Some(e.into()),
                            inodes: None,
                        },
                    )
                }),
            )
            .unwrap();
        });

        b.iter(|| {
            transaction(&mut conn, |tx| {
                quota_limit::with_quota_id_list(
                    tx,
                    (1..=BENCH_QUOTA_ID_NUM).map(|e| e.into()),
                    1.into(),
                    QuotaIDType::User,
                )
                .unwrap();
            });

            counter += 1;
        })
    }

    #[bench]
    fn bench_quota_limits_write(b: &mut Bencher) {
        let mut conn = setup_on_disk_db();
        let mut counter = 0;

        b.iter(|| {
            transaction(&mut conn, |tx| {
                update(
                    tx,
                    (1..=BENCH_QUOTA_ID_NUM).map(|e| {
                        (
                            QuotaIDType::User,
                            1.into(),
                            SpaceAndInodeLimits {
                                quota_id: (e + counter).into(),
                                space: Some((e + counter).into()),
                                inodes: None,
                            },
                        )
                    }),
                )
                .unwrap();
            });

            counter += 1;
        })
    }
}
