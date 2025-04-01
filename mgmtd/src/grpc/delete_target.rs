use super::*;
use shared::bee_msg::misc::RefreshCapacityPools;
use shared::bee_msg::storage_pool::RefreshStoragePools;

/// Deletes a target
pub(crate) async fn delete_target(
    ctx: Context,
    req: pm::DeleteTargetRequest,
) -> Result<pm::DeleteTargetResponse> {
    fail_on_pre_shutdown(&ctx)?;

    let target: EntityId = required_field(req.target)?.try_into()?;
    let execute: bool = required_field(req.execute)?;

    let target = ctx
        .db
        .conn(move |conn| {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

            let target = target.resolve(&tx, EntityType::Target)?;

            if target.node_type() != NodeType::Storage {
                bail!("Only storage targets can be deleted directly");
            }

            let assigned_groups: usize = tx.query_row_cached(
                sql!(
                    "SELECT COUNT(*) FROM buddy_groups_ext
                    WHERE p_target_uid = ?1 OR s_target_uid = ?1"
                ),
                [target.uid],
                |row| row.get(0),
            )?;

            if assigned_groups > 0 {
                bail!("Target {target} is part of a buddy group");
            }

            db::target::delete_storage(&tx, target.num_id().try_into()?)?;

            if execute {
                tx.commit()?;
            }
            Ok(target)
        })
        .await?;

    if execute {
        log::info!("Target deleted: {target}");

        notify_nodes(
            &ctx,
            &[NodeType::Meta],
            &RefreshCapacityPools { ack_id: "".into() },
        )
        .await;

        // Storage targets deletion alter pool membership, so trigger an immediate pool refresh
        notify_nodes(
            &ctx,
            &[NodeType::Meta, NodeType::Storage],
            &RefreshStoragePools { ack_id: "".into() },
        )
        .await;
    }

    let target = Some(target.into());

    log::warn!("{target:?}");

    Ok(pm::DeleteTargetResponse { target })
}
