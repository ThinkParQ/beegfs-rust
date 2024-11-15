use super::common::QUOTA_NOT_ENABLED_STR;
use super::*;
use std::cmp::Ordering;

pub(crate) async fn set_default_quota_limits(
    app: &impl App,
    req: pm::SetDefaultQuotaLimitsRequest,
) -> Result<pm::SetDefaultQuotaLimitsResponse> {
    fail_on_missing_license(app, LicensedFeature::Quota)?;
    fail_on_pre_shutdown(app)?;

    if !app.static_info().user_config.quota_enable {
        bail!(QUOTA_NOT_ENABLED_STR);
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

    app.write_tx(move |tx| {
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
