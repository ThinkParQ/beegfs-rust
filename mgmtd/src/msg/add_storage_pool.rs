use super::*;
use shared::types::{EntityType, NodeType, NodeTypeServer};

pub(super) async fn handle(
    msg: msg::AddStoragePool,
    ctx: &Context,
    _req: &impl Request,
) -> msg::AddStoragePoolResp {
    match ctx
        .db
        .op(move |tx| {
            let alias = &std::str::from_utf8(&msg.alias)?;

            // Check alias is free
            if db::entity::get_uid(tx, alias)?.is_some() {
                bail!(TypedError::value_exists("Alias", alias));
            }

            // Check all of the given target IDs exist
            db::target::validate_ids(tx, &msg.move_target_ids, NodeTypeServer::Storage)?;
            // Check all of the given buddy group IDs exist
            db::buddy_group::validate_ids(tx, &msg.move_buddy_group_ids, NodeTypeServer::Storage)?;

            let pool_id = if msg.pool_id != 0 {
                // Check given pool_id is free
                if db::storage_pool::get_uid(tx, msg.pool_id)?.is_some() {
                    bail!(TypedError::value_exists("storage pool ID", msg.pool_id));
                }

                msg.pool_id
            } else {
                db::misc::find_new_id(tx, "storage_pools", "pool_id", 1..=0xFFFF)?
            };

            // Insert entity then storage pool entry
            let new_uid = db::entity::insert(tx, EntityType::StoragePool, alias)?;
            db::storage_pool::insert(tx, pool_id, new_uid)?;

            // Update storage pool assignments for the given targets
            db::target::update_storage_pools(tx, pool_id, &msg.move_target_ids)?;
            db::buddy_group::update_storage_pools(tx, pool_id, &msg.move_buddy_group_ids)?;

            Ok(pool_id)
        })
        .await
    {
        Ok(actual_id) => {
            log::info!(
                "Added new storage pool with ID {} (Requested: {})",
                actual_id,
                msg.pool_id,
            );

            notify_nodes(
                ctx,
                &[NodeType::Meta, NodeType::Storage],
                &msg::RefreshStoragePools { ack_id: "".into() },
            )
            .await;

            msg::AddStoragePoolResp {
                result: OpsErr::SUCCESS,
                pool_id: actual_id,
            }
        }
        Err(err) => {
            log_error_chain!(err, "Adding storage pool with ID {} failed", msg.pool_id);

            msg::AddStoragePoolResp {
                result: match err.downcast_ref() {
                    Some(TypedError::ValueExists { .. }) => OpsErr::EXISTS,
                    _ => OpsErr::INTERNAL,
                },
                pool_id: 0,
            }
        }
    }
}
