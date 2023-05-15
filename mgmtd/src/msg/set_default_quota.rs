use super::*;

pub(super) async fn handle(
    msg: msg::SetDefaultQuota,
    rcc: impl RequestConnectionController,
    ci: impl ComponentInteractor,
) -> Result<()> {
    match ci
        .execute_db(move |tx| {
            db::quota_default_limits::update(
                tx,
                msg.pool_id,
                msg.id_type,
                match msg.space {
                    Space::ZERO => None,
                    n => Some(n),
                },
                match msg.inodes {
                    Inodes::ZERO => None,
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
            rcc.respond(&msg::SetDefaultQuotaResp { result: true })
                .await
        }

        Err(err) => {
            log::error!(
                "Setting default quota of type {:?} for storage pool {} failed:\n{:?}",
                msg.id_type,
                msg.pool_id,
                err
            );
            rcc.respond(&msg::SetDefaultQuotaResp { result: false })
                .await
        }
    }
}
