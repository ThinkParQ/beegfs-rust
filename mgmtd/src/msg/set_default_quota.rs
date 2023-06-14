use super::*;

pub(super) async fn handle(
    msg: msg::SetDefaultQuota,
    ci: impl ComponentInteractor,
    _rcc: &impl RequestConnectionController,
) -> msg::SetDefaultQuotaResp {
    match ci
        .execute_db(move |tx| {
            // Check pool ID exists
            if db::storage_pool::get_uid(tx, msg.pool_id)?.is_none() {
                return Err(DbError::value_not_found("storage pool ID", msg.pool_id));
            }

            match msg.space {
                0 => {
                    db::quota_default_limit::delete(tx, msg.pool_id, msg.id_type, QuotaType::Space)?
                }
                n => db::quota_default_limit::update(
                    tx,
                    msg.pool_id,
                    msg.id_type,
                    QuotaType::Space,
                    n,
                )?,
            };

            match msg.inodes {
                0 => db::quota_default_limit::delete(
                    tx,
                    msg.pool_id,
                    msg.id_type,
                    QuotaType::Inodes,
                )?,
                n => db::quota_default_limit::update(
                    tx,
                    msg.pool_id,
                    msg.id_type,
                    QuotaType::Inodes,
                    n,
                )?,
            };

            Ok(())
        })
        .await
    {
        Ok(_) => {
            log::info!(
                "Set default quota of type {:?} for storage pool {}",
                msg.id_type,
                msg.pool_id,
            );
            msg::SetDefaultQuotaResp { result: true }
        }

        Err(err) => {
            log_error_chain!(
                err,
                "Setting default quota of type {:?} for storage pool {} failed",
                msg.id_type,
                msg.pool_id
            );
            msg::SetDefaultQuotaResp { result: false }
        }
    }
}
