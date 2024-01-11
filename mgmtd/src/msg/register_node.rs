use super::*;
use crate::db::node_nic::ReplaceNic;
use crate::types::{EntityType, NodeType};
use db::misc::MetaRoot;
use shared::msg::heartbeat::Heartbeat;
use shared::msg::register_node::{RegisterNode, RegisterNodeResp};
use shared::types::NodeID;
use std::net::SocketAddr;
use std::sync::Arc;

pub(super) async fn handle(
    msg: RegisterNode,
    ctx: &Context,
    _req: &impl Request,
) -> RegisterNodeResp {
    let node_id = update(msg, ctx).await;

    RegisterNodeResp {
        node_num_id: node_id,
    }
}

/// Processes incoming node information. Registeres new nodes if config allows it
pub(super) async fn update(msg: RegisterNode, ctx: &Context) -> NodeID {
    let msg2 = msg.clone();
    let info = ctx.info;

    let res: Result<_> = async {
        let db_res = ctx
            .db
            .op(move |tx| {
                let alias = &std::str::from_utf8(&msg.node_alias)?;
                let node_type = msg.node_type.try_into()?;

                let node_uid = if msg.node_id == 0 {
                    // No node ID given => new node
                    None
                } else {
                    // If Some is returned, the node with node_id already exists
                    db::node::get_uid(tx, msg.node_id, node_type)?
                };

                let (node_id, node_uid) = if let Some(node_uid) = node_uid {
                    // Existing node, update data
                    db::entity::update_alias(tx, node_uid, alias)?;
                    db::node::update(tx, node_uid, msg.port)?;

                    (msg.node_id, node_uid)
                } else {
                    // New node, do additional checks and insert data

                    // Check node registration is allowed
                    if !info.user_config.registration_enable {
                        bail!("Registration of new nodes is not allowed");
                    }

                    // Check alias doesnt exist yet
                    if db::entity::get_uid(tx, alias)?.is_some() {
                        bail!(TypedError::value_exists("Alias", alias));
                    };

                    // Services send a 0 value when they want the new node to be assigned an ID
                    // automatically
                    let node_id = if msg.node_id == 0 {
                        db::misc::find_new_id(
                            tx,
                            &format!("{}_nodes", node_type.as_sql_str()),
                            "node_id",
                            1..=0xFFFF,
                        )?
                    } else {
                        msg.node_id
                    };

                    // Insert new entity and node entry
                    let node_uid = db::entity::insert(tx, EntityType::Node, alias)?;
                    db::node::insert(tx, node_id, node_uid, node_type, msg.port)?;

                    // if this is a meta node, auto-add a corresponding meta target after the node.
                    // This is required because currently the rest of BeeGFS
                    // doesn't know about meta targets and expects exactly one
                    // meta target per meta node (with the same ID)
                    if node_type == NodeType::Meta {
                        db::target::insert_meta(tx, node_id, &format!("{alias}_target"))?;
                    }

                    (node_id, node_uid)
                };

                // Update the corresponding nic lists
                db::node_nic::replace(
                    tx,
                    node_uid,
                    msg.nics.iter().map(|e| ReplaceNic {
                        nic_type: e.nic_type.into(),
                        addr: &e.addr,
                        name: std::str::from_utf8(&e.name).unwrap_or("INVALID_UTF8"),
                    }),
                )?;

                Ok((
                    node_uid,
                    node_id,
                    match node_type {
                        // In case this is a meta node, the requestor expects info about the meta
                        // root
                        NodeType::Meta => db::misc::get_meta_root(tx)?,
                        _ => MetaRoot::Unknown,
                    },
                ))
            })
            .await?;

        ctx.conn.replace_node_addrs(
            db_res.0,
            msg2.nics
                .clone()
                .into_iter()
                .map(|e| SocketAddr::new(e.addr.into(), msg.port))
                .collect::<Arc<_>>(),
        );

        Ok(db_res)
    }
    .await;

    match res {
        Ok((_node_uid, node_id, meta_root)) => {
            log::info!(
                "Processed {:?} node info from with ID {} (Requested: {})",
                msg2.node_type,
                node_id,
                msg2.node_id,
            );

            // notify all nodes
            notify_nodes(
                ctx,
                match msg.node_type {
                    shared::types::NodeType::Meta => &[NodeType::Meta, NodeType::Client],
                    shared::types::NodeType::Storage => {
                        &[NodeType::Meta, NodeType::Storage, NodeType::Client]
                    }
                    shared::types::NodeType::Client => &[NodeType::Meta],
                    _ => &[],
                },
                &Heartbeat {
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
                "Processing {:?} node info for ID {} failed",
                msg.node_type,
                msg.node_id,
            );

            0
        }
    }
}
