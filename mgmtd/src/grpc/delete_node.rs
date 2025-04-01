use super::*;
use shared::bee_msg::node::RemoveNode;

/// Deletes a node. If it is a meta node, deletes its target first.
pub(crate) async fn delete_node(
    app: &impl App,
    req: pm::DeleteNodeRequest,
) -> Result<pm::DeleteNodeResponse> {
    fail_on_pre_shutdown(app)?;

    let node: EntityId = required_field(req.node)?.try_into()?;
    let execute: bool = required_field(req.execute)?;

    let node = app
        .db_conn(move |conn| {
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

        app.send_notifications(
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
#[cfg(test)]
mod test {
    use super::*;
    use crate::app::test::*;

    #[tokio::test]
    async fn delete_node() {
        let h = TestApp::new().await;
        let mut req = pm::DeleteNodeRequest {
            node: Some(pb::EntityIdSet {
                uid: None,
                alias: None,
                legacy_id: Some(pb::LegacyId {
                    num_id: 1,
                    node_type: pb::NodeType::Management.into(),
                }),
            }),
            execute: Some(true),
        };

        // Can't delete management node
        super::delete_node(&h, req.clone()).await.unwrap_err();

        // Can't delete meta buddy group member target (which is on the node)
        req.node.as_mut().unwrap().uid = None;
        req.node.as_mut().unwrap().legacy_id = Some(pb::LegacyId {
            num_id: 1,
            node_type: pb::NodeType::Meta.into(),
        });
        super::delete_node(&h, req.clone()).await.unwrap_err();

        // Delete empty node
        req.node.as_mut().unwrap().legacy_id = Some(pb::LegacyId {
            num_id: 99,
            node_type: pb::NodeType::Meta.into(),
        });
        let resp = super::delete_node(&h, req.clone()).await.unwrap();

        assert_eq!(resp.node.unwrap().legacy_id.unwrap().num_id, 99);
        assert_eq_db!(
            h,
            "SELECT COUNT(*) FROM nodes WHERE node_id = ?1 AND node_type = ?2",
            [99, NodeType::Meta.sql_variant()],
            0
        );
    }
}
