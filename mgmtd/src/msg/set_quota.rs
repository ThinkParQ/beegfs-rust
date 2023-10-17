use super::*;
use crate::db::quota_limit::SpaceAndInodeLimits;
use shared::msg::set_quota::{SetQuota, SetQuotaResp};

pub(super) async fn handle(msg: SetQuota, ctx: &Context, _req: &impl Request) -> SetQuotaResp {
    match ctx
        .db
        .op(move |tx| {
            // Check pool ID exists
            if db::storage_pool::get_uid(tx, msg.pool_id)?.is_none() {
                bail!(TypedError::value_not_found("storage pool ID", msg.pool_id));
            }

            db::quota_limit::update(
                tx,
                msg.quota_entry.into_iter().map(|e| {
                    (
                        e.id_type.into(),
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
            SetQuotaResp { result: 1 }
        }

        Err(err) => {
            log_error_chain!(err, "Setting quota for storage pool {} failed", msg.pool_id);

            SetQuotaResp { result: 0 }
        }
    }
}
