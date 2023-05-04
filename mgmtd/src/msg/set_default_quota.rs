use super::*;

pub(super) async fn handle(
    msg: msg::SetDefaultQuota,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    match hnd
        .execute_db(move |tx| {
            db::quota_default_limits::update(
                tx,
                msg.pool_id,
                msg.id_type,
                match msg.space {
                    QuotaSpace::ZERO => None,
                    n => Some(n),
                },
                match msg.inodes {
                    QuotaInodes::ZERO => None,
                    n => Some(n),
                },
            )
        })
        .await
    {
        Ok(_) => {
            log::info!(
                "Set default quota of type {:?} for storage pool {}",
                msg.id_type,
                msg.pool_id,
            );
            chn.respond(&msg::SetDefaultQuotaResp { result: true })
                .await
        }

        Err(err) => {
            log::error!(
                "Setting default quota of type {:?} for storage pool {} failed:\n{:?}",
                msg.id_type,
                msg.pool_id,
                err
            );
            chn.respond(&msg::SetDefaultQuotaResp { result: false })
                .await
        }
    }
}
