use super::*;

pub(super) async fn handle(
    msg: msg::AddStoragePool,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    match hnd
        .execute_db(move |tx| {
            let id = db::storage_pools::insert(
                tx,
                if msg.id == StoragePoolID::ZERO {
                    None
                } else {
                    Some(msg.id)
                },
                &msg.alias,
            )?;

            db::targets::update_storage_pools(tx, id, msg.move_target_ids)?;
            db::buddy_groups::update_storage_pools(tx, id, msg.move_buddy_group_ids)?;

            Ok(id)
        })
        .await
    {
        Ok(actual_id) => {
            log::info!(
                "Added new storage pool with ID {} (Requested: {})",
                actual_id,
                msg.id,
            );

            hnd.notify_nodes(&msg::RefreshStoragePools { ack_id: "".into() })
                .await;

            chn.respond(&msg::AddStoragePoolResp {
                result: OpsErr::SUCCESS,
                pool_id: actual_id,
            })
            .await?;
        }
        Err(err) => {
            log::error!("Adding storage pool with ID {} failed:\n{:?}", msg.id, err);

            chn.respond(&msg::AddStoragePoolResp {
                result: OpsErr::EXISTS,
                pool_id: StoragePoolID::ZERO,
            })
            .await?;
        }
    }

    Ok(())
}
