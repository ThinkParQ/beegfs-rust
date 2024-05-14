use super::*;
use crate::types::SqliteStr;
use pb::beegfs::beegfs as pb;

pub(crate) async fn get(
    ctx: &Context,
    _req: GetBuddyGroupsRequest,
) -> Result<GetBuddyGroupsResponse> {
    let buddy_groups = ctx
        .db
        .op(|tx| {
            Ok(tx.query_map_collect(
                sql!(
                    "SELECT buddy_group_uid, buddy_group_id, bg.alias, bg.node_type,
                        p_target_uid, p_t.target_id, p_t.alias,
                        s_target_uid, s_t.target_id, s_t.alias,
                        sp.pool_uid, bg.pool_id, e_sp.alias,
                        p_t.consistency, s_t.consistency
                    FROM all_buddy_groups_v AS bg
                    INNER JOIN all_targets_v AS p_t ON p_t.target_uid = p_target_uid
                    INNER JOIN all_targets_v AS s_t ON s_t.target_uid = s_target_uid
                    LEFT JOIN storage_pools AS sp ON sp.pool_id = bg.pool_id
                    LEFT JOIN entities AS e_sp ON e_sp.uid = sp.pool_uid"
                ),
                [],
                |row| {
                    let node_type = pb::NodeType::from_row(row, 3)? as i32;

                    Ok(pb::get_buddy_groups_response::BuddyGroup {
                        id: Some(pb::EntityIdSet {
                            uid: row.get(0)?,
                            legacy_id: Some(pb::LegacyId {
                                num_id: row.get(1)?,
                                node_type,
                                entity_type: pb::EntityType::BuddyGroup as i32,
                            }),
                            alias: row.get(2)?,
                        }),
                        node_type,
                        primary_target: Some(pb::EntityIdSet {
                            uid: row.get(4)?,
                            legacy_id: Some(pb::LegacyId {
                                num_id: row.get(5)?,
                                node_type,
                                entity_type: pb::EntityType::Target as i32,
                            }),
                            alias: row.get(6)?,
                        }),
                        secondary_target: Some(pb::EntityIdSet {
                            uid: row.get(7)?,
                            legacy_id: Some(pb::LegacyId {
                                num_id: row.get(8)?,
                                node_type,
                                entity_type: pb::EntityType::Target as i32,
                            }),
                            alias: row.get(9)?,
                        }),
                        storage_pool: if let Some(uid) = row.get::<_, Option<u64>>(10)? {
                            Some(pb::EntityIdSet {
                                uid,
                                legacy_id: Some(pb::LegacyId {
                                    num_id: row.get(11)?,
                                    node_type,
                                    entity_type: pb::EntityType::StoragePool as i32,
                                }),
                                alias: row.get(12)?,
                            })
                        } else {
                            None
                        },
                        primary_consistency_state: pb::ConsistencyState::from_row(row, 13)? as i32,
                        secondary_consistency_state: pb::ConsistencyState::from_row(row, 14)?
                            as i32,
                    })
                },
            )?)
        })
        .await?;

    Ok(GetBuddyGroupsResponse { buddy_groups })
}
