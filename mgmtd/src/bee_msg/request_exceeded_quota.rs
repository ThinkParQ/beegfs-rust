use super::*;
use crate::db::quota_usage::PoolOrTargetId;
use shared::bee_msg::quota::*;

impl HandleWithResponse for RequestExceededQuota {
    type Response = RequestExceededQuotaResp;

    fn error_response() -> Self::Response {
        RequestExceededQuotaResp {
            result: OpsErr::INTERNAL,
            inner: SetExceededQuota::default(),
        }
    }

    async fn handle(self, app: &impl App, _req: &mut impl Request) -> Result<Self::Response> {
        let inner = app
            .read_tx(move |tx| {
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
            .await?;

        Ok(RequestExceededQuotaResp {
            result: OpsErr::SUCCESS,
            inner,
        })
    }
}
