use super::*;
use crate::db::quota_limits::SpaceAndInodeLimits;

pub(super) async fn handle(
    msg: msg::SetQuota,
    rcc: impl RequestConnectionController,
    ci: impl ComponentInteractor,
) -> Result<()> {
    match ci
        .execute_db(move |tx| {
            db::quota_limits::update(
                tx,
                msg.quota_entry.into_iter().map(|e| {
                    (
                        e.id_type,
                        msg.pool_id,
                        SpaceAndInodeLimits {
                            quota_id: e.id,
                            space: match e.space {
                                Space::ZERO => None,
                                n => Some(n),
                            },
                            inodes: match e.inodes {
                                Inodes::ZERO => None,
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
            rcc.respond(&msg::SetQuotaResp { result: true }).await
        }

        Err(err) => {
            log::error!(
                "Setting quota for storage pool {} failed:\n{:?}",
                msg.pool_id,
                err
            );
            rcc.respond(&msg::SetQuotaResp { result: false }).await
        }
    }
}
