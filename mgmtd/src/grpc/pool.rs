use super::*;
use rusqlite::{named_params, Row};
use shared::bee_msg::storage_pool::RefreshStoragePools;

/// Delivers the list of pools
pub(crate) async fn get(ctx: Context, req: pm::GetPoolsRequest) -> Result<pm::GetPoolsResponse> {
    let (mut pools, targets, buddy_groups) = ctx
        .db
        .read_tx(move |tx| {
            let make_sp = |row: &Row| -> rusqlite::Result<pm::get_pools_response::StoragePool> {
                Ok(pm::get_pools_response::StoragePool {
                    id: Some(pb::EntityIdSet {
                        uid: row.get(0)?,
                        legacy_id: Some(pb::LegacyId {
                            num_id: row.get(1)?,
                            node_type: pb::NodeType::Storage.into(),
                        }),
                        alias: row.get(2)?,
                    }),
                    ..Default::default()
                })
            };

            let pools: Vec<_> = if req.with_quota_limits {
                tx.query_map_collect(
                    sql!(
                        "SELECT p.pool_uid, p.pool_id, alias,
                            qus.value, qui.value, qgs.value, qgi.value
                        FROM storage_pools AS p
                        INNER JOIN entities ON uid = pool_uid
                        LEFT JOIN quota_default_limits AS qus ON qus.pool_id = p.pool_id
                            AND qus.id_type = :user AND qus.quota_type = :space
                        LEFT JOIN quota_default_limits AS qui ON qui.pool_id = p.pool_id
                            AND qui.id_type = :user AND qui.quota_type = :inode
                        LEFT JOIN quota_default_limits AS qgs ON qgs.pool_id = p.pool_id
                            AND qgs.id_type = :group AND qgs.quota_type = :space
                        LEFT JOIN quota_default_limits AS qgi ON qgi.pool_id = p.pool_id
                            AND qgi.id_type = :group AND qgi.quota_type = :inode"
                    ),
                    named_params![
                        ":user": QuotaIdType::User.sql_variant(),
                        ":group": QuotaIdType::Group.sql_variant(),
                        ":space": QuotaType::Space.sql_variant(),
                        ":inode": QuotaType::Inode.sql_variant()
                    ],
                    |row| {
                        let mut sp = make_sp(row)?;
                        sp.user_space_limit = row.get::<_, Option<i64>>(3)?.or(Some(-1));
                        sp.user_inode_limit = row.get::<_, Option<i64>>(4)?.or(Some(-1));
                        sp.group_space_limit = row.get::<_, Option<i64>>(5)?.or(Some(-1));
                        sp.group_inode_limit = row.get::<_, Option<i64>>(6)?.or(Some(-1));
                        Ok(sp)
                    },
                )?
            } else {
                tx.query_map_collect(
                    sql!(
                        "SELECT pool_uid, pool_id, alias
                        FROM storage_pools
                        INNER JOIN entities ON uid = pool_uid"
                    ),
                    [],
                    make_sp,
                )?
            };

            let targets: Vec<(Uid, _)> = tx.query_map_collect(
                sql!(
                    "SELECT target_uid, target_id, alias, pool_uid
                    FROM storage_targets
                    INNER JOIN entities ON uid = target_uid
                    INNER JOIN pools USING(node_type, pool_id)"
                ),
                [],
                |row| {
                    Ok((
                        row.get(3)?,
                        pb::EntityIdSet {
                            uid: row.get(0)?,
                            legacy_id: Some(pb::LegacyId {
                                num_id: row.get(1)?,
                                node_type: pb::NodeType::Storage.into(),
                            }),
                            alias: row.get(2)?,
                        },
                    ))
                },
            )?;

            let buddy_groups: Vec<(Uid, _)> = tx.query_map_collect(
                sql!(
                    "SELECT group_uid, group_id, alias, pool_uid
                    FROM storage_buddy_groups
                    INNER JOIN entities ON uid = group_uid
                    INNER JOIN pools USING(pool_id)"
                ),
                [],
                |row| {
                    Ok((
                        row.get(3)?,
                        pb::EntityIdSet {
                            uid: row.get(0)?,
                            legacy_id: Some(pb::LegacyId {
                                num_id: row.get(1)?,
                                node_type: pb::NodeType::Storage.into(),
                            }),
                            alias: row.get(2)?,
                        },
                    ))
                },
            )?;

            Ok((pools, targets, buddy_groups))
        })
        .await?;

    // Merge pool, target and buddy group lists together
    for p in &mut pools {
        for t in &targets {
            if p.id.as_ref().is_some_and(|e| e.uid == Some(t.0)) {
                p.targets.push(t.1.clone());
            }
        }

        for t in &buddy_groups {
            if p.id.as_ref().is_some_and(|e| e.uid == Some(t.0)) {
                p.buddy_groups.push(t.1.clone());
            }
        }
    }

    Ok(pm::GetPoolsResponse { pools })
}

/// Creates a new pool, optionally assigning targets and groups
pub(crate) async fn create(
    ctx: Context,
    req: pm::CreatePoolRequest,
) -> Result<pm::CreatePoolResponse> {
    needs_license(&ctx, LicensedFeature::Storagepool)?;
    fail_on_pre_shutdown(&ctx)?;

    if req.node_type() != pb::NodeType::Storage {
        bail!("node type must be storage");
    }

    let alias: Alias = required_field(req.alias)?.try_into()?;
    let num_id: PoolId = req.num_id.unwrap_or_default().try_into()?;

    let (pool_uid, alias, pool_id) = ctx
        .db
        .write_tx(move |tx| {
            let (pool_uid, pool_id) = db::storage_pool::insert(tx, num_id, &alias)?;
            assign_pool(tx, pool_id, req.targets, req.buddy_groups)?;
            Ok((pool_uid, alias, pool_id))
        })
        .await?;

    let pool = EntityIdSet {
        uid: pool_uid,
        alias,
        legacy_id: LegacyId {
            node_type: NodeType::Storage,
            num_id: pool_id.into(),
        },
    };

    log::info!("Pool created: {pool}");

    notify_nodes(
        &ctx,
        &[NodeType::Meta, NodeType::Storage],
        &RefreshStoragePools { ack_id: "".into() },
    )
    .await;

    Ok(pm::CreatePoolResponse {
        pool: Some(pool.into()),
    })
}

/// Assigns a pool to a list of targets and buddy groups.
pub(crate) async fn assign(
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
            assign_pool(tx, pool.num_id().try_into()?, req.targets, req.buddy_groups)?;
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
fn assign_pool(
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

/// Deletes a pool. The pool must be empty.
pub(crate) async fn delete(
    ctx: Context,
    req: pm::DeletePoolRequest,
) -> Result<pm::DeletePoolResponse> {
    needs_license(&ctx, LicensedFeature::Storagepool)?;
    fail_on_pre_shutdown(&ctx)?;

    let pool: EntityId = required_field(req.pool)?.try_into()?;
    let execute: bool = required_field(req.execute)?;

    let pool = ctx
        .db
        .conn(move |conn| {
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

        notify_nodes(
            &ctx,
            &[NodeType::Meta, NodeType::Storage],
            &RefreshStoragePools { ack_id: "".into() },
        )
        .await;
    }

    Ok(pm::DeletePoolResponse {
        pool: Some(pool.into()),
    })
}
