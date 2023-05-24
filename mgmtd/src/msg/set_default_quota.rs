use super::*;

pub(super) async fn handle(
    msg: msg::SetDefaultQuota,
    rcc: impl RequestConnectionController,
    ci: impl ComponentInteractor,
) -> Result<()> {
    match ci
        .execute_db(move |tx| {
            match msg.space {
                0 => db::quota_default_limits::delete(
                    tx,
                    msg.pool_id,
                    msg.id_type,
                    QuotaType::Space,
                )?,
                n => db::quota_default_limits::update(
                    tx,
                    msg.pool_id,
                    msg.id_type,
                    QuotaType::Space,
                    n,
                )?,
            };

            match msg.inodes {
                0 => db::quota_default_limits::delete(
                    tx,
                    msg.pool_id,
                    msg.id_type,
                    QuotaType::Inodes,
                )?,
                n => db::quota_default_limits::update(
                    tx,
                    msg.pool_id,
                    msg.id_type,
                    QuotaType::Inodes,
                    n,
                )?,
            };

            Ok(())
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
