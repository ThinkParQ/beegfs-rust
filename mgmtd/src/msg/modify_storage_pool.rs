use super::*;

pub(super) async fn handle(
    msg: msg::ModifyStoragePool,
    ci: impl ComponentInteractor,
    _rcc: &impl RequestConnectionController,
) -> msg::ModifyStoragePoolResp {
    match async {
        ci.db_op(move |tx| {
            // Check ID exists
            let uid = db::storage_pool::get_uid(tx, msg.pool_id)?
                .ok_or_else(|| DbError::value_not_found("storage pool ID", msg.pool_id))?;

            // Check all of the given target IDs exist
            db::target::check_existence(tx, &msg.add_target_ids, NodeTypeServer::Storage)?;
            db::target::check_existence(tx, &msg.remove_target_ids, NodeTypeServer::Storage)?;
            db::buddy_group::check_existence(
                tx,
                &msg.add_buddy_group_ids,
                NodeTypeServer::Storage,
            )?;
            db::buddy_group::check_existence(
                tx,
                &msg.remove_buddy_group_ids,
                NodeTypeServer::Storage,
            )?;

            if let Some(ref new_alias) = msg.alias {
                // Check alias is free
                if db::entity::get_uid(tx, new_alias)?.is_some() {
                    return Err(DbError::value_exists("Alias", new_alias));
                }

                db::entity::update_alias(tx, uid, new_alias)?;
            }

            // Move given target IDs to the given pool or the default pool
            db::target::update_storage_pools(tx, msg.pool_id, &msg.add_target_ids)?;
            db::target::update_storage_pools(tx, StoragePoolID::DEFAULT, &msg.remove_target_ids)?;

            // Same with buddy groups
            db::buddy_group::update_storage_pools(tx, msg.pool_id, &msg.add_buddy_group_ids)?;
            db::buddy_group::update_storage_pools(
                tx,
                StoragePoolID::DEFAULT,
                &msg.remove_buddy_group_ids,
            )?;

            Ok(())
        })
        .await
    }
    .await
    {
        Ok(_) => {
            log::info!("Storage pool {} modified", msg.pool_id,);

            ci.notify_nodes(&msg::RefreshStoragePools { ack_id: "".into() })
                .await;

            msg::ModifyStoragePoolResp {
                result: OpsErr::SUCCESS,
            }
        }
        Err(err) => {
            log_error_chain!(err, "Modifying storage pool {} failed", msg.pool_id);

            msg::ModifyStoragePoolResp {
                result: match err {
                    // Yes, returning OpsErr::INVAL is intended for value not found. Unlike
                    // remove_storage_pool, here this signals that pool ID is invalid
                    DbError::ValueNotFound { .. } => OpsErr::INVAL,
                    _ => OpsErr::INTERNAL,
                },
            }
        }
    }
}
