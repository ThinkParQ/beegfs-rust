use super::*;
use shared::bee_msg::misc::RefreshCapacityPools;

/// Deletes a target
pub(crate) async fn delete_target(
    app: &impl App,
    req: pm::DeleteTargetRequest,
) -> Result<pm::DeleteTargetResponse> {
    fail_on_pre_shutdown(app)?;

    let target: EntityId = required_field(req.target)?.try_into()?;
    let execute: bool = required_field(req.execute)?;

    let target = app
        .db_conn(move |conn| {
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

        app.beemsg_send_notifications(
            &[NodeType::Meta],
            &RefreshCapacityPools { ack_id: "".into() },
        )
        .await;
    }

    let target = Some(target.into());

    log::warn!("{target:?}");

    Ok(pm::DeleteTargetResponse { target })
}
