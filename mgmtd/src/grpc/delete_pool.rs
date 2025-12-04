use super::*;
use shared::bee_msg::storage_pool::RefreshStoragePools;

/// Deletes a pool. The pool must be empty.
pub(crate) async fn delete_pool(
    app: &impl App,
    req: pm::DeletePoolRequest,
) -> Result<pm::DeletePoolResponse> {
    fail_on_missing_license(app, LicensedFeature::Storagepool)?;
    fail_on_pre_shutdown(app)?;

    let pool: EntityId = required_field(req.pool)?.try_into()?;
    let execute: bool = required_field(req.execute)?;

    let pool = app
        .db_conn(move |conn| {
            let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

            let pool = pool.resolve(&tx, EntityType::Pool)?;

            let assigned_targets: usize = tx.query_row(
                sql!("SELECT COUNT(*) FROM storage_targets WHERE pool_id = ?1"),
                [pool.num_id()],
                |row| row.get(0),
            )?;

            let assigned_buddy_groups: usize = tx.query_row(
                sql!("SELECT COUNT(*) FROM storage_buddy_groups WHERE pool_id = ?1"),
                [pool.num_id()],
                |row| row.get(0),
            )?;

            if assigned_targets > 0 || assigned_buddy_groups > 0 {
                bail!(
                    "{assigned_targets} targets and {assigned_buddy_groups} buddy groups \
are still assigned to this pool"
                )
            }

            let affected = tx.execute(sql!("DELETE FROM pools WHERE pool_uid = ?1"), [pool.uid])?;
            check_affected_rows(affected, [1])?;

            if execute {
                tx.commit()?;
            }
            Ok(pool)
        })
        .await?;

    if execute {
        log::info!("Pool deleted: {pool}");

        app.send_notifications(
            &[NodeType::Meta, NodeType::Storage],
            &RefreshStoragePools { ack_id: "".into() },
        )
        .await;
    }

    Ok(pm::DeletePoolResponse {
        pool: Some(pool.into()),
    })
}
