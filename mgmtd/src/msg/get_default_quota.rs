use super::*;
use shared::msg::types::QuotaDefaultLimits;

pub(super) async fn handle(
    msg: msg::GetDefaultQuota,
    ci: impl ComponentInteractor,
    _rcc: &impl RequestConnectionController,
) -> msg::GetDefaultQuotaResp {
    match ci
        .db_op(move |tx| {
            // Check pool ID exists
            if db::storage_pool::get_uid(tx, msg.pool_id)?.is_none() {
                return Err(DbError::value_not_found("storage pool ID", msg.pool_id));
            }

            let res = db::quota_default_limit::with_pool_id(tx, msg.pool_id)?;

            Ok(res)
        })
        .await
    {
        Ok(res) => msg::GetDefaultQuotaResp {
            limits: QuotaDefaultLimits {
                user_space_limit: res.user_space_limit.unwrap_or_default(),
                user_inode_limit: res.user_inodes_limit.unwrap_or_default(),
                group_space_limit: res.group_space_limit.unwrap_or_default(),
                group_inode_limit: res.group_inodes_limit.unwrap_or_default(),
            },
        },
        Err(err) => {
            log_error_chain!(
                err,
                "Getting default quota info for storage pool {} failed",
                msg.pool_id
            );

            msg::GetDefaultQuotaResp {
                limits: QuotaDefaultLimits::default(),
            }
        }
    }
}
