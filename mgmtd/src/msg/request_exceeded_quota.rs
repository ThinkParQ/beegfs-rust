use super::*;
use crate::db::quota_usage::PoolOrTargetID;
use shared::msg::request_exceeded_quota::{RequestExceededQuota, RequestExceededQuotaResp};
use shared::msg::set_exceeded_quota::SetExceededQuota;

pub(super) async fn handle(
    msg: RequestExceededQuota,
    ctx: &Context,
    _req: &impl Request,
) -> RequestExceededQuotaResp {
    match ctx
        .db
        .op(move |tx| {
            let exceeded_ids = db::quota_usage::exceeded_quota_ids(
                tx,
                if msg.pool_id != 0 {
                    PoolOrTargetID::PoolID(msg.pool_id)
                } else {
                    PoolOrTargetID::TargetID(msg.target_id)
                },
                msg.id_type.into(),
                msg.quota_type.into(),
            )?;

            Ok(SetExceededQuota {
                pool_id: msg.pool_id,
                id_type: msg.id_type,
                quota_type: msg.quota_type,
                exceeded_quota_ids: exceeded_ids,
            })
        })
        .await
    {
        Ok(inner) => RequestExceededQuotaResp {
            result: OpsErr::SUCCESS,
            inner,
        },
        Err(err) => {
            log_error_chain!(
                err,
                "Fetching exceeded quota IDs for storage pool {} or target {} failed",
                msg.pool_id,
                msg.target_id
            );
            RequestExceededQuotaResp {
                result: OpsErr::INTERNAL,
                inner: SetExceededQuota::default(),
            }
        }
    }
}
