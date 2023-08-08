use super::*;
use crate::config::registration_enable;
use db::entity::EntityType;
use db::misc::MetaRoot;
use std::net::SocketAddr;
use std::sync::Arc;

pub(super) async fn handle(
    msg: msg::RegisterNode,
    ctx: &impl AppContext,
    _req: &impl Request,
) -> msg::RegisterNodeResp {
    let node_id = update(msg, ctx).await;

    msg::RegisterNodeResp {
        node_num_id: node_id,
    }
}

/// Processes incoming node information. Registeres new nodes if config allows it
pub(super) async fn update(msg: msg::RegisterNode, ctx: &impl AppContext) -> NodeID {
    let msg2 = msg.clone();

    let res: Result<_> = try {
        let db_res = ctx
            .db_op(move |tx| {
                let node_uid = if msg.node_id == NodeID::ZERO {
                    // No node ID given => new node
                    None
                } else {
                    // If Some is returned, the node with node_id already exists
                    db::node::get_uid(tx, msg.node_id, msg.node_type)?
                };

                let (node_id, node_uid) = if let Some(node_uid) = node_uid {
                    // Existing node, update data
                    db::entity::update_alias(tx, node_uid, &msg.node_alias)?;
                    db::node::update(tx, node_uid, msg.port)?;

                    (msg.node_id, node_uid)
                } else {
                    // New node, do additional checks and insert data

                    // Check node registration is allowed
                    if !db::config::get::<registration_enable>(tx)? {
                        return Err(DbError::other("Registration of new nodes is not allowed"));
                    }

                    // Check alias doesnt exist yet
                    if db::entity::get_uid(tx, &msg.node_alias)?.is_some() {
                        return Err(DbError::value_exists("Alias", &msg.node_alias));
                    };

                    // Services send a 0 value when they want the new node to be assigned an ID
                    // automatically
                    let node_id = if msg.node_id == NodeID::ZERO {
                        db::misc::find_new_id(
                            tx,
                            &format!("{}_nodes", msg.node_type.as_sql_str()),
                            "node_id",
                            1..=0xFFFF,
                        )?
                        .into()
                    } else {
                        msg.node_id
                    };

                    // Insert new entity and node entry
                    let node_uid = db::entity::insert(tx, EntityType::Node, &msg.node_alias)?;
                    db::node::insert(tx, node_id, node_uid, msg.node_type, msg.port)?;

                    // if this is a meta node, auto-add a corresponding meta target after the node.
                    // This is required because currently the rest of BeeGFS
                    // doesn't know about meta targets and expects exactly one
                    // meta target per meta node (with the same ID)
                    if msg.node_type == NodeType::Meta {
                        db::target::insert_meta(
                            tx,
                            u16::from(node_id).into(),
                            &format!("{}_target", msg.node_alias).into(),
                        )?;
                    }

                    (node_id, node_uid)
                };

                // Update the corresponding nic lists
                db::node_nic::replace(tx, node_uid, &msg.nics)?;

                Ok((
                    node_uid,
                    node_id,
                    match msg.node_type {
                        // In case this is a meta node, the requestor expects info about the meta
                        // root
                        NodeType::Meta => db::misc::get_meta_root(tx)?,
                        _ => MetaRoot::Unknown,
                    },
                ))
            })
            .await?;

        ctx.replace_node_addrs(
            db_res.0,
            msg2.nics
                .clone()
                .into_iter()
                .map(|e| SocketAddr::new(e.addr.into(), msg.port.into()))
                .collect::<Arc<_>>(),
        );

        db_res
    };

    match res {
        Ok((_node_uid, node_id, meta_root)) => {
            log::info!(
                "Processed {} node info from with ID {} (Requested: {})",
                msg2.node_type,
                node_id,
                msg2.node_id,
            );

            // notify all nodes
            ctx.notify_nodes(
                match msg.node_type {
                    NodeType::Meta => &[NodeType::Meta, NodeType::Client],
                    NodeType::Storage => &[NodeType::Meta, NodeType::Storage, NodeType::Client],
                    NodeType::Client => &[NodeType::Meta],
                    _ => &[],
                },
                &msg::Heartbeat {
                    instance_version: 0,
                    nic_list_version: 0,
                    node_type: msg2.node_type,
                    node_alias: msg2.node_alias,
                    ack_id: "".into(),
                    node_num_id: node_id,
                    root_num_id: match meta_root {
                        MetaRoot::Unknown => 0,
                        MetaRoot::Normal(node_id, _) => node_id.into(),
                        MetaRoot::Mirrored(buddy_group_id) => buddy_group_id.into(),
                    },
                    is_root_mirrored: match meta_root {
                        MetaRoot::Unknown | MetaRoot::Normal(_, _) => 0,
                        MetaRoot::Mirrored(_) => 1,
                    },
                    port: msg.port,
                    port_tcp_unused: msg.port,
                    nic_list: msg2.nics,
                },
            )
            .await;

            node_id
        }

        Err(err) => {
            log_error_chain!(
                err,
                "Processing {} node info for ID {} failed",
                msg.node_type,
                msg.node_id,
            );

            NodeID::ZERO
        }
    }
}
