use super::*;
use shared::bee_msg::node::RemoveNode;

/// Deletes a node. If it is a meta node, deletes its target first.
pub(crate) async fn delete_node(
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
