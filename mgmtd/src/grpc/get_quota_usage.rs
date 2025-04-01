use super::common::{QUOTA_NOT_ENABLED_STR, QUOTA_STREAM_BUF_SIZE, QUOTA_STREAM_PAGE_LIMIT};
use super::*;
use itertools::Itertools;
use std::fmt::Write;

pub(crate) async fn get_quota_usage(
    ctx: Context,
    req: pm::GetQuotaUsageRequest,
) -> Result<RespStream<pm::GetQuotaUsageResponse>> {
    needs_license(&ctx, LicensedFeature::Quota)?;

    if !ctx.info.user_config.quota_enable {
        bail!(QUOTA_NOT_ENABLED_STR);
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

    let stream = resp_stream(QUOTA_STREAM_BUF_SIZE, async move |stream| {
        let mut offset = 0;

        loop {
            let sql = sql.clone();
            let entries: Vec<_> = ctx
                .db
                .read_tx(move |tx| {
                    tx.query_map_collect(&sql, [offset, QUOTA_STREAM_PAGE_LIMIT], |row| {
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
            if len < QUOTA_STREAM_PAGE_LIMIT {
                return Ok(());
            }

            offset += QUOTA_STREAM_PAGE_LIMIT;
        }
    });

    Ok(stream)
}
