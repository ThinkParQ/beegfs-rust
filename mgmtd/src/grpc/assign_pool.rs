use super::*;
use shared::bee_msg::storage_pool::RefreshStoragePools;

/// Assigns a pool to a list of targets and buddy groups.
pub(crate) async fn assign_pool(
    ctx: Context,
    req: pm::AssignPoolRequest,
) -> Result<pm::AssignPoolResponse> {
    needs_license(&ctx, LicensedFeature::Storagepool)?;
    fail_on_pre_shutdown(&ctx)?;

    let pool: EntityId = required_field(req.pool)?.try_into()?;

    let pool = ctx
        .db
        .write_tx(move |tx| {
            let pool = pool.resolve(tx, EntityType::Pool)?;
            do_assign(tx, pool.num_id().try_into()?, req.targets, req.buddy_groups)?;
            Ok(pool)
        })
        .await?;

    log::info!("Pool assigned: {pool}");

    notify_nodes(
        &ctx,
        &[NodeType::Meta, NodeType::Storage],
        &RefreshStoragePools { ack_id: "".into() },
    )
    .await;

    Ok(pm::AssignPoolResponse {
        pool: Some(pool.into()),
    })
}

/// Do the actual assign work
pub(super) fn do_assign(
    tx: &Transaction,
    pool_id: PoolId,
    targets: Vec<pb::EntityIdSet>,
    groups: Vec<pb::EntityIdSet>,
) -> Result<()> {
    // Target being part of a buddy group can not be assigned individually
    let mut check_group_membership = tx.prepare_cached(sql!(
        "SELECT COUNT(*) FROM storage_buddy_groups AS g
        INNER JOIN targets AS p_t ON p_t.target_id = g.p_target_id AND p_t.node_type = g.node_type
        INNER JOIN targets AS s_t ON s_t.target_id = g.s_target_id AND s_t.node_type = g.node_type
        WHERE p_t.target_uid = ?1 OR s_t.target_uid = ?1"
    ))?;

    let mut assign_target = tx.prepare_cached(sql!(
        "UPDATE targets SET pool_id = ?1 WHERE target_uid = ?2"
    ))?;

    // Do the checks and assign for each target in the given list. This is expensive, but shouldn't
    // matter as this command should only be run occasionally and not with very large lists of
    // targets.
    for t in targets {
        let eid = EntityId::try_from(t)?;
        let target = eid.resolve(tx, EntityType::Target)?;
        if check_group_membership.query_row([target.uid], |row| row.get::<_, i64>(0))? > 0 {
            bail!("Target {eid} can't be assigned directly as it's part of a buddy group");
        }

        assign_target.execute(params![pool_id, target.uid])?;
    }

    let mut assign_group = tx.prepare_cached(sql!(
        "UPDATE buddy_groups SET pool_id = ?1 WHERE group_uid = ?2"
    ))?;

    // Targets being part of buddy groups are auto-assigned to the new pool
    let mut assign_grouped_targets = tx.prepare_cached(sql!(
        "UPDATE targets SET pool_id = ?1
        FROM (
            SELECT p_t.target_uid AS p_uid, s_t.target_uid AS s_uid FROM buddy_groups AS g
            INNER JOIN targets AS p_t ON p_t.target_id = g.p_target_id AND p_t.node_type = g.node_type
            INNER JOIN targets AS s_t ON s_t.target_id = g.s_target_id AND s_t.node_type = g.node_type
            WHERE group_uid = ?2
        )
        WHERE target_uid IN (p_uid, s_uid)"
    ))?;

    // Assign each group and their targets to the new pool
    for g in groups {
        let eid = EntityId::try_from(g)?;
        let group = eid.resolve(tx, EntityType::BuddyGroup)?;

        assign_group.execute(params![pool_id, group.uid])?;
        assign_grouped_targets.execute(params![pool_id, group.uid])?;
    }

    Ok(())
}
