use super::*;
use itertools::Itertools;
use std::cmp::Ordering;
use std::fmt::Write;

const QUOTA_NOT_ENABLED: &str = "Quota support is not enabled";

pub(crate) async fn set_default_quota_limits(
    ctx: Context,
    req: pm::SetDefaultQuotaLimitsRequest,
) -> Result<pm::SetDefaultQuotaLimitsResponse> {
    needs_license(&ctx, LicensedFeature::Quota)?;
    fail_on_pre_shutdown(&ctx)?;

    if !ctx.info.user_config.quota_enable {
        bail!(QUOTA_NOT_ENABLED);
    }

    let pool: EntityId = required_field(req.pool)?.try_into()?;

    fn update(
        tx: &Transaction,
        limit: i64,
        pool_id: PoolId,
        id_type: QuotaIdType,
        quota_type: QuotaType,
    ) -> Result<()> {
        match limit.cmp(&-1) {
            Ordering::Less => bail!("invalid {id_type} {quota_type} limit {limit}"),
            Ordering::Equal => {
                tx.execute_cached(
                    sql!(
                        "DELETE FROM quota_default_limits
                        WHERE pool_id = ?1 AND id_type = ?2 AND quota_type = ?3"
                    ),
                    params![pool_id, id_type.sql_variant(), quota_type.sql_variant()],
                )?;
            }
            Ordering::Greater => {
                tx.execute_cached(
                    sql!(
                        "REPLACE INTO quota_default_limits (pool_id, id_type, quota_type, value)
                        VALUES(?1, ?2, ?3, ?4)"
                    ),
                    params![
                        pool_id,
                        id_type.sql_variant(),
                        quota_type.sql_variant(),
                        limit
                    ],
                )?;
            }
        }

        Ok(())
    }

    ctx.db
        .write_tx(move |tx| {
            let pool = pool.resolve(tx, EntityType::Pool)?;
            let pool_id: PoolId = pool.num_id().try_into()?;

            if let Some(l) = req.user_space_limit {
                update(tx, l, pool_id, QuotaIdType::User, QuotaType::Space)?;
            }
            if let Some(l) = req.user_inode_limit {
                update(tx, l, pool_id, QuotaIdType::User, QuotaType::Inode)?;
            }
            if let Some(l) = req.group_space_limit {
                update(tx, l, pool_id, QuotaIdType::Group, QuotaType::Space)?;
            }
            if let Some(l) = req.group_inode_limit {
                update(tx, l, pool_id, QuotaIdType::Group, QuotaType::Inode)?;
            }

            Ok(())
        })
        .await?;

    Ok(pm::SetDefaultQuotaLimitsResponse {})
}

pub(crate) async fn set_quota_limits(
    ctx: Context,
    req: pm::SetQuotaLimitsRequest,
) -> Result<pm::SetQuotaLimitsResponse> {
    needs_license(&ctx, LicensedFeature::Quota)?;
    fail_on_pre_shutdown(&ctx)?;

    if !ctx.info.user_config.quota_enable {
        bail!(QUOTA_NOT_ENABLED);
    }

    ctx.db
        .write_tx(|tx| {
            let mut insert_stmt = tx.prepare_cached(sql!(
                "REPLACE INTO quota_limits
                (quota_id, id_type, quota_type, pool_id, value)
                VALUES (?1, ?2, ?3, ?4, ?5)"
            ))?;

            let mut delete_stmt = tx.prepare_cached(sql!(
                "DELETE FROM quota_limits
                WHERE quota_id = ?1 AND id_type = ?2 AND quota_type = ?3 AND pool_id = ?4"
            ))?;

            for lim in req.limits {
                let id_type: QuotaIdType = lim.id_type().try_into()?;
                let quota_id = required_field(lim.quota_id)?;

                let pool: EntityId = required_field(lim.pool)?.try_into()?;
                let pool_id = pool.resolve(tx, EntityType::Pool)?.num_id();

                if let Some(l) = lim.space_limit {
                    if l > -1 {
                        insert_stmt.execute(params![
                            quota_id,
                            id_type.sql_variant(),
                            QuotaType::Space.sql_variant(),
                            pool_id,
                            l
                        ])?
                    } else {
                        delete_stmt.execute(params![
                            quota_id,
                            id_type.sql_variant(),
                            QuotaType::Space.sql_variant(),
                            pool_id,
                        ])?
                    };
                }

                if let Some(l) = lim.inode_limit {
                    if l > -1 {
                        insert_stmt.execute(params![
                            quota_id,
                            id_type.sql_variant(),
                            QuotaType::Inode.sql_variant(),
                            pool_id,
                            l
                        ])?
                    } else {
                        delete_stmt.execute(params![
                            quota_id,
                            id_type.sql_variant(),
                            QuotaType::Inode.sql_variant(),
                            pool_id,
                        ])?
                    };
                }
            }

            Ok(())
        })
        .await?;

    Ok(pm::SetQuotaLimitsResponse {})
}

// Fetching pages of 1M from quota_usage takes around 2100ms on my slow developer laptop (using a
// release build). In comparison, a page size of 100k takes around 750ms which is far worse. This
// feels like a good middle point to not let the requester wait too long and not waste too many db
// thread cycles with overhead.
const PAGE_LIMIT: usize = 1_000_000;
// Need to hit a compromise between memory footprint and speed. Bigger is better if multiple
// pages need to be fetched but doesn't matter too much if not. Each entry is roughly 50 -
// 60 bytes, so 100k (= 5-6MB) feels fine. And it is still big enough to give a significant
// boost to throughput for big numbers.
const BUF_SIZE: usize = 100_000;

pub(crate) async fn get_quota_limits(
    ctx: Context,
    req: pm::GetQuotaLimitsRequest,
) -> Result<RespStream<pm::GetQuotaLimitsResponse>> {
    needs_license(&ctx, LicensedFeature::Quota)?;

    if !ctx.info.user_config.quota_enable {
        bail!(QUOTA_NOT_ENABLED);
    }

    let pool_id = if let Some(pool) = req.pool {
        let pool: EntityId = pool.try_into()?;
        let pool_id = ctx
            .db
            .read_tx(move |tx| pool.resolve(tx, EntityType::Pool))
            .await?
            .num_id();
        Some(pool_id)
    } else {
        None
    };

    let mut r#where = "FALSE ".to_string();

    let mut filter =
        |min: Option<u32>, max: Option<u32>, list: &[u32], typ: QuotaIdType| -> Result<()> {
            if min.is_some() || max.is_some() || !list.is_empty() {
                write!(r#where, "OR (l.id_type = {} ", typ.sql_variant())?;

                if min.is_some() || max.is_some() {
                    write!(
                        r#where,
                        "AND l.quota_id BETWEEN {} AND {} ",
                        min.unwrap_or(0),
                        max.unwrap_or(u32::MAX)
                    )?;
                }
                if !list.is_empty() {
                    write!(r#where, "AND l.quota_id IN ({}) ", list.iter().join(","))?;
                }
                if let Some(pool_id) = pool_id {
                    write!(r#where, "AND l.pool_id = {pool_id} ")?;
                }

                write!(r#where, ") ")?;
            }

            Ok(())
        };

    filter(
        req.user_id_min,
        req.user_id_max,
        &req.user_id_list,
        QuotaIdType::User,
    )?;

    filter(
        req.group_id_min,
        req.group_id_max,
        &req.group_id_list,
        QuotaIdType::Group,
    )?;

    let sql = format!(
        "SELECT l.quota_id, l.id_type, l.pool_id, sp.alias, sp.pool_uid,
            MAX(CASE WHEN l.quota_type = {space} THEN l.value END) AS space_limit,
            MAX(CASE WHEN l.quota_type = {inode} THEN l.value END) AS inode_limit
        FROM quota_limits AS l
        INNER JOIN pools_ext AS sp USING(node_type, pool_id)
        WHERE {where}
        GROUP BY l.quota_id, l.id_type, l.pool_id
        LIMIT ?1, ?2",
        space = QuotaType::Space.sql_variant(),
        inode = QuotaType::Inode.sql_variant()
    );

    let stream = resp_stream(BUF_SIZE, async move |stream| {
        let mut offset = 0;

        loop {
            let sql = sql.clone();
            let entries: Vec<_> = ctx
                .db
                .read_tx(move |tx| {
                    tx.query_map_collect(&sql, [offset, PAGE_LIMIT], |row| {
                        Ok(pm::QuotaInfo {
                            pool: Some(pb::EntityIdSet {
                                uid: row.get(4)?,
                                legacy_id: Some(pb::LegacyId {
                                    num_id: row.get(2)?,
                                    node_type: pb::NodeType::Storage.into(),
                                }),
                                alias: row.get(3)?,
                            }),
                            id_type: QuotaIdType::from_row(row, 1)?.into_proto_i32(),
                            quota_id: Some(row.get(0)?),
                            space_limit: row.get(5)?,
                            inode_limit: row.get(6)?,
                            space_used: None,
                            inode_used: None,
                        })
                    })
                    .map_err(Into::into)
                })
                .await?;

            let len = entries.len();

            // Send the entries to the client
            for entry in entries {
                stream
                    .send(pm::GetQuotaLimitsResponse {
                        limits: Some(entry),
                    })
                    .await?;
            }

            // This was the last page? Then we are done
            if len < PAGE_LIMIT {
                return Ok(());
            }

            offset += PAGE_LIMIT;
        }
    });

    Ok(stream)
}

pub(crate) async fn get_quota_usage(
    ctx: Context,
    req: pm::GetQuotaUsageRequest,
) -> Result<RespStream<pm::GetQuotaUsageResponse>> {
    needs_license(&ctx, LicensedFeature::Quota)?;

    if !ctx.info.user_config.quota_enable {
        bail!(QUOTA_NOT_ENABLED);
    }

    let mut r#where = "FALSE ".to_string();

    let mut filter =
        |min: Option<u32>, max: Option<u32>, list: &[u32], typ: QuotaIdType| -> Result<()> {
            if min.is_some() || max.is_some() || !list.is_empty() {
                write!(r#where, "OR (u.id_type = {} ", typ.sql_variant())?;

                if min.is_some() || max.is_some() {
                    write!(
                        r#where,
                        "AND u.quota_id BETWEEN {} AND {} ",
                        min.unwrap_or(0),
                        max.unwrap_or(u32::MAX)
                    )?;
                }
                if !list.is_empty() {
                    write!(r#where, "AND u.quota_id IN ({}) ", list.iter().join(","))?;
                }

                write!(r#where, ") ")?;
            }

            Ok(())
        };

    filter(
        req.user_id_min,
        req.user_id_max,
        &req.user_id_list,
        QuotaIdType::User,
    )?;

    filter(
        req.group_id_min,
        req.group_id_max,
        &req.group_id_list,
        QuotaIdType::Group,
    )?;

    let mut having = "TRUE ".to_string();

    if let Some(pool) = req.pool {
        let pool: EntityId = pool.try_into()?;
        let pool_uid = ctx
            .db
            .read_tx(move |tx| pool.resolve(tx, EntityType::Pool))
            .await?
            .uid;

        write!(having, "AND sp.pool_uid = {pool_uid} ")?;
    }
    if let Some(exceeded) = req.exceeded {
        let base = "(space_used > space_limit AND space_limit > -1
                OR inode_used > inode_limit AND inode_limit > -1)";
        if exceeded {
            write!(having, "AND {base} ")?;
        } else {
            write!(having, "AND NOT {base} ")?;
        }
    }

    let sql = format!(
        "SELECT u.quota_id, u.id_type, sp.pool_id, sp.alias, sp.pool_uid,
            MAX(CASE WHEN u.quota_type = {space} THEN
                COALESCE(l.value, d.value, -1)
            END) AS space_limit,
            MAX(CASE WHEN u.quota_type = {inode} THEN
                COALESCE(l.value, d.value, -1)
            END) AS inode_limit,
            SUM(CASE WHEN u.quota_type = {space} THEN u.value END) AS space_used,
            SUM(CASE WHEN u.quota_type = {inode} THEN u.value END) AS inode_used
        FROM quota_usage AS u
        INNER JOIN targets AS st USING(node_type, target_id)
        INNER JOIN pools_ext AS sp USING(node_type, pool_id)
        LEFT JOIN quota_default_limits AS d USING(id_type, quota_type, pool_id)
        LEFT JOIN quota_limits AS l USING(quota_id, id_type, quota_type, pool_id)
        WHERE {where}
        GROUP BY u.quota_id, u.id_type, st.pool_id
        HAVING {having}
        LIMIT ?1, ?2",
        space = QuotaType::Space.sql_variant(),
        inode = QuotaType::Inode.sql_variant()
    );

    let stream = resp_stream(BUF_SIZE, async move |stream| {
        let mut offset = 0;

        loop {
            let sql = sql.clone();
            let entries: Vec<_> = ctx
                .db
                .read_tx(move |tx| {
                    tx.query_map_collect(&sql, [offset, PAGE_LIMIT], |row| {
                        Ok(pm::QuotaInfo {
                            pool: Some(pb::EntityIdSet {
                                uid: row.get(4)?,
                                legacy_id: Some(pb::LegacyId {
                                    num_id: row.get(2)?,
                                    node_type: pb::NodeType::Storage.into(),
                                }),
                                alias: row.get(3)?,
                            }),
                            id_type: QuotaIdType::from_row(row, 1)?.into_proto_i32(),
                            quota_id: Some(row.get(0)?),
                            space_limit: row.get(5)?,
                            inode_limit: row.get(6)?,
                            space_used: row.get(7)?,
                            inode_used: row.get(8)?,
                        })
                    })
                    .map_err(Into::into)
                })
                .await?;

            let len = entries.len();
            let mut entries = entries.into_iter();

            // If this if the first entry, include the quota refresh period. Do not send it again
            // after to minimize message size.
            if offset == 0 {
                if let Some(entry) = entries.next() {
                    stream
                        .send(pm::GetQuotaUsageResponse {
                            entry: Some(entry),
                            refresh_period_s: Some(
                                ctx.info.user_config.quota_update_interval.as_secs(),
                            ),
                        })
                        .await?;
                }
            }

            // Send all the (remaining) entries to the client
            for entry in entries {
                stream
                    .send(pm::GetQuotaUsageResponse {
                        entry: Some(entry),
                        refresh_period_s: None,
                    })
                    .await?;
            }

            // This was the last page? Then we are done
            if len < PAGE_LIMIT {
                return Ok(());
            }

            offset += PAGE_LIMIT;
        }
    });

    Ok(stream)
}
