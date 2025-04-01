use super::common::QUOTA_NOT_ENABLED_STR;
use super::*;

pub(crate) async fn set_quota_limits(
    ctx: Context,
    req: pm::SetQuotaLimitsRequest,
) -> Result<pm::SetQuotaLimitsResponse> {
    needs_license(&ctx, LicensedFeature::Quota)?;
    fail_on_pre_shutdown(&ctx)?;

    if !ctx.info.user_config.quota_enable {
        bail!(QUOTA_NOT_ENABLED_STR);
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
