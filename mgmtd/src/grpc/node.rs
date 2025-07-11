use super::*;
use shared::bee_msg::node::RemoveNode;

/// Delivers a list of nodes
pub(crate) async fn get(ctx: Context, req: pm::GetNodesRequest) -> Result<pm::GetNodesResponse> {
    let (mut nodes, nics, meta_root_node, meta_root_buddy_group, fs_uuid) = ctx
        .db
        .read_tx(move |tx| {
            // Fetching the nic list is optional as it causes additional load
            let nics: Vec<(Uid, pm::get_nodes_response::node::Nic)> = if req.include_nics {
                tx.prepare_cached(sql!(
                    "SELECT nn.node_uid, nn.addr, n.port, nn.nic_type, nn.name
                    FROM node_nics AS nn
                    INNER JOIN nodes AS n USING(node_uid)
                    ORDER BY nn.node_uid ASC"
                ))?
                .query_and_then([], |row| {
                    let nic_type = NicType::from_row(row, 3)?.into_proto_i32();

                    Ok((
                        row.get(0)?,
                        pm::get_nodes_response::node::Nic {
                            addr: row.get(1)?,
                            name: row.get(4)?,
                            nic_type,
                        },
                    ))
                })?
                .collect::<Result<Vec<_>>>()?
            } else {
                vec![]
            };

            // Fetch the node list
            let nodes: Vec<pm::get_nodes_response::Node> = tx.query_map_collect(
                sql!("SELECT node_uid, node_id, node_type, alias, port FROM nodes_ext"),
                [],
                |row| {
                    let node_type = NodeType::from_row(row, 2)?.into_proto_i32();

                    let node = pb::EntityIdSet {
                        uid: row.get(0)?,
                        legacy_id: Some(pb::LegacyId {
                            num_id: row.get(1)?,
                            node_type,
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

            // Figure out the meta root node and buddy mirror information
            let maybe_row = tx
                .query_row_cached(
                    sql!(
                        "SELECT
                            COALESCE(mn.node_uid, mn2.node_uid),
                            COALESCE(e.alias, e2.alias),
                            COALESCE(mn.node_id, mn2.node_id),
                            mg.group_id,
                            mg.group_uid,
                            ge.alias
                        FROM root_inode as ri
                        LEFT JOIN targets AS mt USING(node_type, target_id)
                        LEFT JOIN nodes AS mn ON mn.node_id = mt.node_id
                            AND mn.node_type = mt.node_type
                        LEFT JOIN entities AS e ON e.uid = mn.node_uid
                        LEFT JOIN buddy_groups AS mg USING(node_type, group_id)
                        LEFT JOIN entities AS ge ON ge.uid = mg.group_uid
                        LEFT JOIN targets AS mt2 ON mt2.target_id = mg.p_target_id
                            AND mt2.node_type = mg.node_type
                        LEFT JOIN nodes AS mn2 ON mn2.node_id = mt2.node_id
                            AND mn2.node_type = mg.node_type
                        LEFT JOIN entities AS e2 ON e2.uid = mn2.node_uid"
                    ),
                    [],
                    |row| {
                        Ok((
                            row.get::<_, Uid>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, NodeId>(2)?,
                            row.get::<_, Option<NodeId>>(3)?,
                            row.get::<_, Option<Uid>>(4)?,
                            row.get::<_, Option<String>>(5)?,
                        ))
                    },
                )
                .optional()?;

            let (meta_root_node, meta_root_buddy_group) =
                if let Some((uid, alias, num_id, bg_num_id, bg_uid, bg_alias)) = maybe_row {
                    let meta_root_node = Some(EntityIdSet {
                        uid,
                        alias: alias.try_into()?,
                        legacy_id: LegacyId {
                            node_type: NodeType::Meta,
                            num_id,
                        },
                    });

                    let meta_root_buddy_group = if let (Some(num_id), Some(uid), Some(alias)) =
                        (bg_num_id, bg_uid, bg_alias)
                    {
                        Some(EntityIdSet {
                            uid,
                            alias: alias.try_into()?,
                            legacy_id: LegacyId {
                                node_type: NodeType::Meta,
                                num_id,
                            },
                        })
                    } else {
                        None
                    };

                    (meta_root_node, meta_root_buddy_group)
                } else {
                    (None, None)
                };

            let fs_uuid = db::config::get(tx, db::config::Config::FsUuid)
                .context("Could not read file system UUID from database")?;

            Ok((nodes, nics, meta_root_node, meta_root_buddy_group, fs_uuid))
        })
        .await?;

    if req.include_nics {
        for node in &mut nodes {
            node.nics = nics
                .iter()
                .filter(|(uid, _)| node.id.as_ref().is_some_and(|e| e.uid == Some(*uid)))
                .cloned()
                .map(|(_, mut nic)| {
                    nic.addr = format!("{}:{}", nic.addr, node.port);
                    nic
                })
                .collect();
        }
    }
    Ok(pm::GetNodesResponse {
        nodes,
        meta_root_node: meta_root_node.map(|e| e.into()),
        meta_root_buddy_group: meta_root_buddy_group.map(|e| e.into()),
        fs_uuid,
    })
}

/// Deletes a node. If it is a meta node, deletes its target first.
pub(crate) async fn delete(
    ctx: Context,
    req: pm::DeleteNodeRequest,
) -> Result<pm::DeleteNodeResponse> {
    fail_on_pre_shutdown(&ctx)?;

    let node: EntityId = required_field(req.node)?.try_into()?;
    let execute: bool = required_field(req.execute)?;

    let node = ctx
        .db
        .conn(move |conn| {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

            let node = node.resolve(&tx, EntityType::Node)?;

            if node.uid == MGMTD_UID {
                bail!("Management node can not be deleted");
            }

            // Meta nodes have an auto-assigned target which needs to be deleted first.
            if node.node_type() == NodeType::Meta {
                let assigned_groups: usize = tx.query_row_cached(
                    sql!(
                        "SELECT COUNT(*) FROM meta_buddy_groups
                        WHERE p_target_id = ?1 OR s_target_id = ?1"
                    ),
                    [node.num_id()],
                    |row| row.get(0),
                )?;

                if assigned_groups > 0 {
                    bail!("The target belonging to meta node {node} is part of a buddy group");
                }

                let target_has_root_inode: usize = tx.query_row(
                    sql!("SELECT COUNT(*) FROM root_inode WHERE target_id = ?1"),
                    [node.num_id()],
                    |row| row.get(0),
                )?;

                if target_has_root_inode > 0 {
                    bail!("The target belonging to meta node {node} has the root inode");
                }

                // There should be exactly one meta target per meta node
                tx.execute(
                    sql!("DELETE FROM targets WHERE node_id = ?1 AND node_type = ?2"),
                    params![node.num_id(), NodeType::Meta.sql_variant()],
                )?;
            } else {
                let assigned_targets: usize = tx.query_row_cached(
                    sql!("SELECT COUNT(*) FROM targets_ext WHERE node_uid = ?1"),
                    [node.uid],
                    |row| row.get(0),
                )?;

                if assigned_targets > 0 {
                    bail!("Node {node} still has targets assigned");
                }
            }

            db::node::delete(&tx, node.uid)?;

            if execute {
                tx.commit()?;
            }
            Ok(node)
        })
        .await?;

    if execute {
        log::info!("Node deleted: {node}");

        notify_nodes(
            &ctx,
            match node.node_type() {
                NodeType::Meta => &[NodeType::Meta, NodeType::Client],
                NodeType::Storage => &[NodeType::Meta, NodeType::Storage, NodeType::Client],
                _ => &[],
            },
            &RemoveNode {
                node_type: node.node_type(),
                node_id: node.num_id(),
                ack_id: "".into(),
            },
        )
        .await;
    }

    Ok(pm::DeleteNodeResponse {
        node: Some(node.into()),
    })
}
