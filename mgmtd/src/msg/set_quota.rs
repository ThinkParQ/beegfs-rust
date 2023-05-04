use super::*;
use crate::db::quota_limits::SpaceAndInodeLimits;

pub(super) async fn handle(
    msg: msg::SetQuota,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    match hnd
        .execute_db(move |tx| {
            db::quota_limits::update_from_iter(
                tx,
                msg.quota_entry.into_iter().map(|e| {
                    (
                        e.id_type,
                        msg.pool_id,
                        SpaceAndInodeLimits {
                            quota_id: e.id,
                            space: match e.space {
                                QuotaSpace::ZERO => None,
                                n => Some(n),
                            },
                            inodes: match e.inodes {
                                QuotaInodes::ZERO => None,
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
            chn.respond(&msg::SetQuotaResp { result: true }).await
        }

        Err(err) => {
            log::error!(
                "Setting quota for storage pool {} failed:\n{:?}",
                msg.pool_id,
                err
            );
            chn.respond(&msg::SetQuotaResp { result: false }).await
        }
    }
}
