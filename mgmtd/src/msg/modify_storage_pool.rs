use super::*;

pub(super) async fn handle(
    msg: msg::ModifyStoragePool,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    match async {
        hnd.execute_db(move |tx| {
            if let Some(alias) = msg.alias {
                db::storage_pools::update_alias(tx, msg.id, &alias)?;
            }

            db::targets::update_storage_pools(tx, msg.id, msg.add_target_ids)?;
            db::targets::update_storage_pools(tx, StoragePoolID::DEFAULT, msg.remove_target_ids)?;

            db::buddy_groups::update_storage_pools(tx, msg.id, msg.add_buddy_group_ids)?;
            db::buddy_groups::update_storage_pools(
                tx,
                StoragePoolID::DEFAULT,
                msg.remove_buddy_group_ids,
            )?;

            Ok(())
        })
        .await?;

        Ok(()) as Result<_>
    }
    .await
    {
        Ok(_) => {
            log::info!("Storage pool {} modified", msg.id,);

            hnd.notify_nodes(&msg::RefreshStoragePools { ack_id: "".into() })
                .await;

            chn.respond(&msg::ModifyStoragePoolResp {
                result: OpsErr::SUCCESS,
            })
            .await
        }
        Err(err) => {
            log::error!("Modifying storage pool {} failed:\n{:?}", msg.id, err);

            chn.respond(&msg::ModifyStoragePoolResp {
                result: OpsErr::INTERNAL,
            })
            .await
        }
    }
}
