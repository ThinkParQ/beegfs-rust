use super::*;

/// Delivers the list of pools
pub(crate) async fn get_pools(
    app: &impl App,
    req: pm::GetPoolsRequest,
) -> Result<pm::GetPoolsResponse> {
    let (mut pools, targets, buddy_groups) = app
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
