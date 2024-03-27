use super::*;
use pb::beegfs::beegfs as pb;

pub(crate) async fn get(
    ctx: &Context,
    _req: GetStoragePoolsRequest,
) -> Result<GetStoragePoolsResponse> {
    let (mut pools, targets, buddy_groups) = ctx
        .db
        .op(|tx| {
            let pools: Vec<_> = tx.query_map_collect(
                sql!(
                    "SELECT p.pool_uid, p.pool_id, e.alias FROM storage_pools AS p
                    INNER JOIN entities AS e ON e.uid = p.pool_uid"
                ),
                [],
                |row| {
                    Ok(pb::get_storage_pools_response::StoragePool {
                        id: Some(pb::EntityIdSet {
                            uid: row.get(0)?,
                            legacy_id: Some(LegacyId {
                                num_id: row.get(1)?,
                                node_type: pb::NodeType::Storage as i32,
                                entity_type: pb::EntityType::StoragePool as i32,
                            }),
                            alias: row.get(2)?,
                        }),
                        targets: vec![],
                        buddy_groups: vec![],
                    })
                },
            )?;

            let targets: Vec<(EntityUID, _)> = tx.query_map_collect(
                sql!(
                    "SELECT target_uid, target_id, alias, pool_uid
                    FROM storage_targets AS st
                    INNER JOIN targets AS t USING(target_uid)
                    INNER JOIN entities AS e ON e.uid = t.target_uid
                    INNER JOIN storage_pools AS p ON p.pool_id = st.pool_id"
                ),
                [],
                |row| {
                    Ok((
                        row.get(3)?,
                        pb::EntityIdSet {
                            uid: row.get(0)?,
                            legacy_id: Some(LegacyId {
                                num_id: row.get(1)?,
                                node_type: pb::NodeType::Storage as i32,
                                entity_type: pb::EntityType::Target as i32,
                            }),
                            alias: row.get(2)?,
                        },
                    ))
                },
            )?;

            let buddy_groups: Vec<(EntityUID, _)> = tx.query_map_collect(
                sql!(
                    "SELECT buddy_group_uid, buddy_group_id, alias, pool_uid
                    FROM storage_buddy_groups AS st
                    INNER JOIN buddy_groups AS t USING(buddy_group_uid)
                    INNER JOIN entities AS e ON e.uid = t.buddy_group_uid
                    INNER JOIN storage_pools AS p ON p.pool_id = st.pool_id"
                ),
                [],
                |row| {
                    Ok((
                        row.get(3)?,
                        pb::EntityIdSet {
                            uid: row.get(0)?,
                            legacy_id: Some(LegacyId {
                                num_id: row.get(1)?,
                                node_type: pb::NodeType::Storage as i32,
                                entity_type: pb::EntityType::Target as i32,
                            }),
                            alias: row.get(2)?,
                        },
                    ))
                },
            )?;

            Ok((pools, targets, buddy_groups))
        })
        .await?;

    for p in &mut pools {
        for t in &targets {
            if p.id.as_ref().is_some_and(|e| e.uid == t.0) {
                p.targets.push(t.1.clone());
            }
        }

        for t in &buddy_groups {
            if p.id.as_ref().is_some_and(|e| e.uid == t.0) {
                p.buddy_groups.push(t.1.clone());
            }
        }
    }

    Ok(GetStoragePoolsResponse { pools })
}
