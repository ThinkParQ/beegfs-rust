use super::*;
use crate::db::quota_entries::PoolOrTargetID;

pub(super) async fn handle(
    msg: msg::RequestExceededQuota,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    match hnd
        .execute_db(move |tx| {
            let exceeded_ids = db::quota_entries::exceeded_quota_ids(
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
        Ok(inner) => {
            chn.respond(&msg::RequestExceededQuotaResp {
                result: OpsErr::SUCCESS,
                inner,
            })
            .await
        }
        Err(err) => {
            log::error!(
                "Fetching exceeded quota IDs for storage pool {} or target {} failed:\n{err:?}",
                msg.pool_id,
                msg.target_id
            );
            chn.respond(&msg::RequestExceededQuotaResp {
                result: OpsErr::INTERNAL,
                inner: msg::SetExceededQuota::default(),
            })
            .await
        }
    }
}
