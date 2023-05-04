use super::*;
use shared::msg::types::QuotaDefaultLimits;

pub(super) async fn handle(
    msg: msg::GetDefaultQuota,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    match hnd
        .execute_db(move |tx| db::quota_default_limits::with_pool_id(tx, msg.pool_id))
        .await
    {
        Ok(res) => {
            chn.respond(&msg::GetDefaultQuotaResp {
                limits: QuotaDefaultLimits {
                    user_space_limit: res.user_space_limit.unwrap_or_default(),
                    user_inode_limit: res.user_inode_limit.unwrap_or_default(),
                    group_space_limit: res.group_space_limit.unwrap_or_default(),
                    group_inode_limit: res.group_inode_limit.unwrap_or_default(),
                },
            })
            .await
        }
        Err(err) => {
            log::error!(
                "Getting default quota info for storage pool {} failed:\n{:?}",
                msg.pool_id,
                err
            );

            chn.respond(&msg::GetDefaultQuotaResp {
                limits: QuotaDefaultLimits::default(),
            })
            .await
        }
    }
}
