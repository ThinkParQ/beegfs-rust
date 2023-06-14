use super::*;
use crate::db::quota_limit::SpaceAndInodeLimits;

pub(super) async fn handle(
    msg: msg::SetQuota,
    ci: impl ComponentInteractor,
    _rcc: &impl RequestConnectionController,
) -> msg::SetQuotaResp {
    match ci
        .execute_db(move |tx| {
            // Check pool ID exists
            if db::storage_pool::get_uid(tx, msg.pool_id)?.is_none() {
                return Err(DbError::value_not_found("storage pool ID", msg.pool_id));
            }

            db::quota_limit::update(
                tx,
                msg.quota_entry.into_iter().map(|e| {
                    (
                        e.id_type,
                        msg.pool_id,
                        SpaceAndInodeLimits {
                            quota_id: e.id,
                            space: match e.space {
                                0 => None,
                                n => Some(n),
                            },
                            inodes: match e.inodes {
                                0 => None,
                                n => Some(n),
                            },
                        },
                    )
                }),
            )
        })
        .await
    {
        Ok(_) => {
            log::info!("Set quota for storage pool {}", msg.pool_id,);
            msg::SetQuotaResp { result: true }
        }

        Err(err) => {
            err.as_ref();
            log_error_chain!(err, "Setting quota for storage pool {} failed", msg.pool_id);

            msg::SetQuotaResp { result: false }
        }
    }
}
