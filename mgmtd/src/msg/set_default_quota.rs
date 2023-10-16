use super::*;
use shared::msg::set_default_quota::{SetDefaultQuota, SetDefaultQuotaResp};
use shared::types::QuotaType;

pub(super) async fn handle(
    msg: SetDefaultQuota,
    ctx: &Context,
    _req: &impl Request,
) -> SetDefaultQuotaResp {
    match ctx
        .db
        .op(move |tx| {
            // Check pool ID exists
            if db::storage_pool::get_uid(tx, msg.pool_id)?.is_none() {
                bail!(TypedError::value_not_found("storage pool ID", msg.pool_id));
            }

            match msg.space {
                0 => {
                    db::quota_default_limit::delete(tx, msg.pool_id, msg.id_type, QuotaType::Space)?
                }
                n => db::quota_default_limit::upsert(
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
                n => db::quota_default_limit::upsert(
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
            SetDefaultQuotaResp { result: 1 }
        }

        Err(err) => {
            log_error_chain!(
                err,
                "Setting default quota of type {:?} for storage pool {} failed",
                msg.id_type,
                msg.pool_id
            );
            SetDefaultQuotaResp { result: 0 }
        }
    }
}
