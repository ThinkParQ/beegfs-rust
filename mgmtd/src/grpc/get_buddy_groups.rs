use super::*;

/// Delivers the list of buddy groups
pub(crate) async fn get_buddy_groups(
    app: &impl App,
    _req: pm::GetBuddyGroupsRequest,
) -> Result<pm::GetBuddyGroupsResponse> {
    let buddy_groups = app
        .read_tx(|tx| {
            Ok(tx.query_map_collect(
                sql!(
                    "SELECT group_uid, group_id, bg.alias AS group_alias, bg.node_type,
                        p_target_uid, p_t.target_id AS p_target_id, p_t.alias AS p_target_alias,
                        s_target_uid, s_t.target_id AS s_target_id, s_t.alias AS s_target_alias,
                        p.pool_uid, bg.pool_id, p.alias AS pool_alias,
                        p_t.consistency AS p_consistency, s_t.consistency AS s_consistency,
                        bg.quota_accounting
                    FROM buddy_groups_ext AS bg
                    INNER JOIN targets_ext AS p_t ON p_t.target_uid = p_target_uid
                    INNER JOIN targets_ext AS s_t ON s_t.target_uid = s_target_uid
                    LEFT JOIN pools_ext AS p USING(node_type, pool_id)"
                ),
                [],
                |row| {
                    let node_type = NodeType::from_row(row, "node_type")?.into_proto_i32();
                    let p_con_state =
                        TargetConsistencyState::from_row(row, "p_consistency")?.into_proto_i32();
                    let s_con_state =
                        TargetConsistencyState::from_row(row, "s_consistency")?.into_proto_i32();

                    Ok(pm::get_buddy_groups_response::BuddyGroup {
                        id: Some(pb::EntityIdSet {
                            uid: row.get("group_uid")?,
                            legacy_id: Some(pb::LegacyId {
                                num_id: row.get("group_id")?,
                                node_type,
                            }),
                            alias: row.get("group_alias")?,
                        }),
                        node_type,
                        primary_target: Some(pb::EntityIdSet {
                            uid: row.get("p_target_uid")?,
                            legacy_id: Some(pb::LegacyId {
                                num_id: row.get("p_target_id")?,
                                node_type,
                            }),
                            alias: row.get("p_target_alias")?,
                        }),
                        secondary_target: Some(pb::EntityIdSet {
                            uid: row.get("s_target_uid")?,
                            legacy_id: Some(pb::LegacyId {
                                num_id: row.get("s_target_id")?,
                                node_type,
                            }),
                            alias: row.get("s_target_alias")?,
                        }),
                        storage_pool: if let Some(uid) = row.get::<_, Option<Uid>>("pool_uid")? {
                            Some(pb::EntityIdSet {
                                uid: Some(uid),
                                legacy_id: Some(pb::LegacyId {
                                    num_id: row.get("pool_id")?,
                                    node_type,
                                }),
                                alias: row.get("pool_alias")?,
                            })
                        } else {
                            None
                        },
                        primary_consistency_state: p_con_state,
                        secondary_consistency_state: s_con_state,
                        options: Some(pm::BuddyGroupOptions {
                            quota_accounting: row.get("quota_accounting")?,
                        }),
                    })
                },
            )?)
        })
        .await?;

    Ok(pm::GetBuddyGroupsResponse { buddy_groups })
}
