use super::*;
use shared::msg::refresh_storage_pools::RefreshStoragePools;
use shared::msg::remove_storage_pool::{RemoveStoragePool, RemoveStoragePoolResp};
use shared::types::DEFAULT_STORAGE_POOL;

pub(super) async fn handle(
    msg: RemoveStoragePool,
    ctx: &Context,
    _req: &impl Request,
) -> RemoveStoragePoolResp {
    match ctx
        .db
        .op(move |tx| {
            // Check ID exists
            db::storage_pool::get_uid(tx, msg.pool_id)?
                .ok_or_else(|| TypedError::value_not_found("storage pool ID", msg.pool_id))?;

            // Check it is not the default pool
            if msg.pool_id == DEFAULT_STORAGE_POOL {
                bail!("The default pool cannot be removed");
            }

            // move all targets in this pool to the default pool
            db::target::reset_storage_pool(tx, msg.pool_id)?;
            db::buddy_group::reset_storage_pool(tx, msg.pool_id)?;

            db::storage_pool::delete(tx, msg.pool_id)?;

            Ok(())
        })
        .await
    {
        Ok(_) => {
            log::info!("Storage pool {} removed", msg.pool_id,);

            notify_nodes(
                ctx,
                &[NodeType::Meta, NodeType::Storage],
                &RefreshStoragePools { ack_id: "".into() },
            )
            .await;

            RemoveStoragePoolResp {
                result: OpsErr::SUCCESS,
            }
        }
        Err(err) => {
            log_error_chain!(err, "Removing storage pool {} failed", msg.pool_id);

            RemoveStoragePoolResp {
                result: match err.downcast_ref() {
                    Some(TypedError::ValueNotFound { .. }) => OpsErr::UNKNOWN_POOL,
                    _ => OpsErr::INTERNAL,
                },
            }
        }
    }
}
