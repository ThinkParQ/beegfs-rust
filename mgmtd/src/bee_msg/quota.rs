use super::*;
use crate::db::quota_usage::PoolOrTargetId;
use shared::bee_msg::quota::*;

impl Handler for RequestExceededQuota {
    type Response = RequestExceededQuotaResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        let res = ctx
            .db
            .op(move |tx| {
                let exceeded_ids = db::quota_usage::exceeded_quota_ids(
                    tx,
                    if self.pool_id != 0 {
                        PoolOrTargetId::PoolID(self.pool_id)
                    } else {
                        PoolOrTargetId::TargetID(self.target_id)
                    },
                    self.id_type,
                    self.quota_type,
                )?;

                Ok(SetExceededQuota {
                    pool_id: self.pool_id,
                    id_type: self.id_type,
                    quota_type: self.quota_type,
                    exceeded_quota_ids: exceeded_ids,
                })
            })
            .await;

        match res {
            Ok(inner) => RequestExceededQuotaResp {
                result: OpsErr::SUCCESS,
                inner,
            },
            Err(err) => {
                log_error_chain!(
                    err,
                    "Fetching exceeded quota ids for storage pool {} or target {} failed",
                    self.pool_id,
                    self.target_id
                );
                RequestExceededQuotaResp {
                    result: OpsErr::INTERNAL,
                    inner: SetExceededQuota::default(),
                }
            }
        }
    }
}
