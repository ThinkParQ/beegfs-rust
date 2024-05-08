use super::*;
use crate::types::SqliteStr;
use protobuf::{beegfs as pb, management as pm};
use shared::types::{NicType, NodeID};
use std::net::Ipv4Addr;

pub(crate) async fn get(ctx: &Context, req: GetNodesRequest) -> Result<GetNodesResponse> {
    let (mut nodes, nics, meta_root_node) = ctx
        .db
        .op(move |tx| {
            let nics: Vec<(EntityUID, pm::get_nodes_response::node::Nic)> = if req.include_nics {
                tx.query_map_collect(
                    sql!(
                        "SELECT nn.node_uid, nn.addr, n.port, nn.nic_type, nn.name
                        FROM node_nics AS nn
                        INNER JOIN nodes AS n USING(node_uid)
                        ORDER BY nn.node_uid ASC"
                    ),
                    [],
                    |row| {
                        let nic_type = NicType::from_row(row, 3)? as i32;

                        Ok((
                            row.get(0)?,
                            pm::get_nodes_response::node::Nic {
                                addr: Ipv4Addr::from(row.get::<_, [u8; 4]>(1)?).to_string(),
                                name: row.get(4)?,
                                nic_type,
                            },
                        ))
                    },
                )?
            } else {
                vec![]
            };

            let nodes: Vec<pm::get_nodes_response::Node> = tx.query_map_collect(
                sql!(
                    "SELECT node_uid, node_id, node_type, alias, port
                    FROM all_nodes_v"
                ),
                [],
                |row| {
                    let node_type = pb::NodeType::from_row(row, 2)? as i32;

                    let node = pb::EntityIdSet {
                        uid: row.get(0)?,
                        legacy_id: Some(pb::LegacyId {
                            num_id: row.get(1)?,
                            node_type,
                            entity_type: pb::EntityType::Node as i32,
                        }),
                        alias: row.get(3)?,
                    };

                    Ok(pm::get_nodes_response::Node {
                        id: Some(node),
                        node_type,
                        port: row.get(4)?,
                        nics: vec![],
                    })
                },
            )?;

            let meta_root_node: (EntityUID, String, NodeID) = tx.query_row(
                sql!(
                    "SELECT
                        COALESCE(mn.node_uid, mn2.node_uid, 0),
                        COALESCE(e.alias, e2.alias, ''),
                        COALESCE(mn.node_id, mn2.node_id, 0)
                    FROM root_inode as ri
                    LEFT JOIN meta_targets AS mt ON mt.target_id = ri.target_id
                    LEFT JOIN meta_nodes AS mn ON mn.node_id = mt.node_id
                    LEFT JOIN entities AS e ON e.uid = mn.node_uid
                    LEFT JOIN meta_buddy_groups AS mg ON mg.buddy_group_id = ri.buddy_group_id
                    LEFT JOIN meta_targets AS mt2 ON mt2.target_id = mg.p_target_id
                    LEFT JOIN meta_nodes AS mn2 ON mn2.node_id = mt2.node_id
                    LEFT JOIN entities AS e2 ON e.uid = mn2.node_uid"
                ),
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;

            Ok((nodes, nics, meta_root_node))
        })
        .await?;

    if req.include_nics {
        for node in &mut nodes {
            node.nics = nics
                .iter()
                .filter(|(uid, _)| node.id.as_ref().is_some_and(|e| e.uid == *uid))
                .cloned()
                .map(|(_, mut nic)| {
                    nic.addr = format!("{}:{}", nic.addr, node.port);
                    nic
                })
                .collect();
        }
    }

    Ok(GetNodesResponse {
        nodes,
        meta_root_node: if meta_root_node.0 != 0 {
            Some(pb::EntityIdSet {
                uid: meta_root_node.0,
                legacy_id: Some(pb::LegacyId {
                    num_id: meta_root_node.2,
                    node_type: pb::NodeType::Meta as i32,
                    entity_type: pb::EntityType::Node as i32,
                }),
                alias: meta_root_node.1,
            })
        } else {
            None
        },
    })
}
