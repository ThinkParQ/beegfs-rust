use super::*;

/// Delivers the list of buddy groups
pub(crate) async fn get_buddy_groups(
    ctx: Context,
    _req: pm::GetBuddyGroupsRequest,
) -> Result<pm::GetBuddyGroupsResponse> {
    let buddy_groups = ctx
        .db
        .read_tx(|tx| {
            Ok(tx.query_map_collect(
                sql!(
                    "SELECT group_uid, group_id, bg.alias, bg.node_type,
                        p_target_uid, p_t.target_id, p_t.alias,
                        s_target_uid, s_t.target_id, s_t.alias,
                        p.pool_uid, bg.pool_id, p.alias,
                        p_t.consistency, s_t.consistency
                    FROM buddy_groups_ext AS bg
                    INNER JOIN targets_ext AS p_t ON p_t.target_uid = p_target_uid
                    INNER JOIN targets_ext AS s_t ON s_t.target_uid = s_target_uid
                    LEFT JOIN pools_ext AS p USING(node_type, pool_id)"
                ),
                [],
                |row| {
                    let node_type = NodeType::from_row(row, 3)?.into_proto_i32();
                    let p_con_state = TargetConsistencyState::from_row(row, 13)?.into_proto_i32();
                    let s_con_state = TargetConsistencyState::from_row(row, 14)?.into_proto_i32();

                    Ok(pm::get_buddy_groups_response::BuddyGroup {
                        id: Some(pb::EntityIdSet {
                            uid: row.get(0)?,
                            legacy_id: Some(pb::LegacyId {
                                num_id: row.get(1)?,
                                node_type,
                            }),
                            alias: row.get(2)?,
                        }),
                        node_type,
                        primary_target: Some(pb::EntityIdSet {
                            uid: row.get(4)?,
                            legacy_id: Some(pb::LegacyId {
                                num_id: row.get(5)?,
                                node_type,
                            }),
                            alias: row.get(6)?,
                        }),
                        secondary_target: Some(pb::EntityIdSet {
                            uid: row.get(7)?,
                            legacy_id: Some(pb::LegacyId {
                                num_id: row.get(8)?,
                                node_type,
                            }),
                            alias: row.get(9)?,
                        }),
                        storage_pool: if let Some(uid) = row.get::<_, Option<Uid>>(10)? {
                            Some(pb::EntityIdSet {
                                uid: Some(uid),
                                legacy_id: Some(pb::LegacyId {
                                    num_id: row.get(11)?,
                                    node_type,
                                }),
                                alias: row.get(12)?,
                            })
                        } else {
                            None
                        },
                        primary_consistency_state: p_con_state,
                        secondary_consistency_state: s_con_state,
                    })
                },
            )?)
        })
        .await?;

    Ok(pm::GetBuddyGroupsResponse { buddy_groups })
}
