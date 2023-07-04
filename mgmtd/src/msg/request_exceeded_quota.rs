use super::*;
use crate::db::quota_entry::PoolOrTargetID;

pub(super) async fn handle(
    msg: msg::RequestExceededQuota,
    ci: impl ComponentInteractor,
    _rcc: &impl RequestConnectionController,
) -> msg::RequestExceededQuotaResp {
    match ci
        .db_op(move |tx| {
            let exceeded_ids = db::quota_entry::exceeded_quota_ids(
                tx,
                if msg.pool_id != StoragePoolID::ZERO {
                    PoolOrTargetID::PoolID(msg.pool_id)
                } else {
                    PoolOrTargetID::TargetID(msg.target_id)
                },
                msg.id_type,
                msg.quota_type,
            )?;

            Ok(msg::SetExceededQuota {
                pool_id: msg.pool_id,
                id_type: msg.id_type,
                quota_type: msg.quota_type,
                exceeded_quota_ids: exceeded_ids,
            })
        })
        .await
    {
        Ok(inner) => msg::RequestExceededQuotaResp {
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
            msg::RequestExceededQuotaResp {
                result: OpsErr::INTERNAL,
                inner: msg::SetExceededQuota::default(),
            }
        }
    }
}
