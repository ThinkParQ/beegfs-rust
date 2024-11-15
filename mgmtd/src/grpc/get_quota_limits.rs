use super::common::{QUOTA_NOT_ENABLED_STR, QUOTA_STREAM_BUF_SIZE, QUOTA_STREAM_PAGE_LIMIT};
use super::*;
use itertools::Itertools;
use std::fmt::Write;

pub(crate) async fn get_quota_limits(
    app: &impl App,
    req: pm::GetQuotaLimitsRequest,
) -> Result<RespStream<pm::GetQuotaLimitsResponse>> {
    fail_on_missing_license(app, LicensedFeature::Quota)?;

    if !app.static_info().user_config.quota_enable {
        bail!(QUOTA_NOT_ENABLED_STR);
    }

    let pool_id = if let Some(pool) = req.pool {
        let pool: EntityId = pool.try_into()?;
        let pool_id = app
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

    let app = app.clone();
    let stream = resp_stream(QUOTA_STREAM_BUF_SIZE, async move |stream| {
        let mut offset = 0;

        loop {
            let sql = sql.clone();
            let entries: Vec<_> = app
                .clone()
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
            if len < QUOTA_STREAM_PAGE_LIMIT {
                return Ok(());
            }

            offset += QUOTA_STREAM_PAGE_LIMIT;
        }
    });

    Ok(stream)
}
