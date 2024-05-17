use super::*;
use crate::db::node_nic::ReplaceNic;
use db::misc::MetaRoot;
use shared::bee_msg::misc::Ack;
use shared::bee_msg::node::*;
use shared::types::{NodeID, TargetID, MGMTD_ID, MGMTD_UID};
use std::net::SocketAddr;
use std::sync::Arc;

impl Handler for GetNodes {
    type Response = GetNodesResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        let res = ctx
            .db
            .op(move |tx| {
                let node_type = self.node_type;
                let res = (
                    db::node::get_with_type(tx, node_type)?,
                    db::node_nic::get_with_type(tx, node_type)?,
                    match self.node_type {
                        shared::types::NodeType::Meta => db::misc::get_meta_root(tx)?,
                        _ => MetaRoot::Unknown,
                    },
                );

                Ok(res)
            })
            .await;

        match res {
            Ok(res) => GetNodesResp {
                nodes: res
                    .0
                    .into_iter()
                    .map(|n| Node {
                        alias: n.alias.into_bytes(),
                        num_id: n.id,
                        nic_list: res
                            .1
                            .iter()
                            .filter(|e| e.node_uid == n.uid)
                            .map(|e| Nic {
                                addr: e.addr,
                                name: e.name.to_string().into_bytes(),
                                nic_type: e.nic_type,
                            })
                            .collect(),
                        port: n.port,
                        _unused_tcp_port: n.port,
                        node_type: n.node_type,
                    })
                    .collect(),
                root_num_id: match res.2 {
                    MetaRoot::Unknown => 0,
                    MetaRoot::Normal(node_id, _) => node_id,
                    MetaRoot::Mirrored(buddy_group_id) => buddy_group_id.into(),
                },
                is_root_mirrored: match res.2 {
                    MetaRoot::Unknown => 0,
                    MetaRoot::Normal(_, _) => 0,
                    MetaRoot::Mirrored(_) => 1,
                },
            },
            Err(err) => {
                log_error_chain!(err, "Getting {:?} node list failed", self.node_type);

                GetNodesResp {
                    nodes: vec![],
                    root_num_id: 0,
                    is_root_mirrored: 0,
                }
            }
        }
    }
}

impl Handler for Heartbeat {
    type Response = Ack;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        let _ = update_node(
            RegisterNode {
                instance_version: self.instance_version,
                nic_list_version: self.nic_list_version,
                node_alias: self.node_alias,
                nics: self.nic_list,
                node_type: self.node_type,
                node_id: self.node_num_id,
                root_num_id: self.root_num_id,
                is_root_mirrored: self.is_root_mirrored,
                port: self.port,
                port_tcp_unused: self.port_tcp_unused,
            },
            ctx,
        )
        .await;

        Ack {
            ack_id: self.ack_id,
        }
    }
}

impl Handler for HeartbeatRequest {
    type Response = Heartbeat;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        let res = ctx
            .db
            .op(|tx| {
                Ok((
                    db::entity::get_alias(tx, MGMTD_UID)?
                        .ok_or_else(|| TypedError::value_not_found("UID", MGMTD_UID))?,
                    db::node_nic::get_with_node(tx, MGMTD_UID)?,
                ))
            })
            .await;

        let (alias, nics) = match res {
            Ok((alias, nics)) => (
                alias,
                nics.iter()
                    .map(|e| Nic {
                        addr: e.addr,
                        name: e.name.clone().into_bytes(),
                        nic_type: shared::types::NicType::Ethernet,
                    })
                    .collect(),
            ),
            Err(err) => {
                log_error_chain!(err, "getting management nics failed");
                ("".to_string(), vec![])
            }
        };

        Heartbeat {
            instance_version: 0,
            nic_list_version: 0,
            node_type: shared::types::NodeType::Management,
            node_alias: alias.into_bytes(),
            ack_id: "".into(),
            node_num_id: MGMTD_ID,
            root_num_id: 0,
            is_root_mirrored: 0,
            port: ctx.info.user_config.beemsg_port,
            port_tcp_unused: ctx.info.user_config.beemsg_port,
            nic_list: nics,
        }
    }
}

impl Handler for RegisterNode {
    type Response = RegisterNodeResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        let node_id = update_node(self, ctx).await;

        RegisterNodeResp {
            node_num_id: node_id,
        }
    }
}

/// Processes incoming node information. Registers new nodes if config allows it
async fn update_node(msg: RegisterNode, ctx: &Context) -> NodeID {
    let msg2 = msg.clone();
    let info = ctx.info;

    let res: Result<_> = async {
        let db_res = ctx
            .db
            .op(move |tx| {
                let node_type = msg.node_type;

                let node_uid = if msg.node_id == 0 {
                    // No node ID given => new node
                    None
                } else {
                    // If Some is returned, the node with node_id already exists
                    db::node::get_uid(tx, msg.node_id, node_type)?
                };

                let (node_id, node_uid) = if let Some(node_uid) = node_uid {
                    // Existing node, update data
                    db::node::update(tx, node_uid, msg.port)?;

                    (msg.node_id, node_uid)
                } else {
                    // New node, do additional checks and insert data

                    // Check node registration is allowed
                    if !info.user_config.registration_enable {
                        bail!("Registration of new nodes is not allowed");
                    }

                    // Insert new node entry
                    let (node_uid, node_id) =
                        db::node::insert(tx, msg.node_id, None, node_type, msg.port)?;

                    // if this is a meta node, auto-add a corresponding meta target after the node.
                    if node_type == NodeType::Meta {
                        // Convert the NodeID to a TargetID. Due to the difference in bitsize, meta
                        // node IDs are not allowed to be bigger than u16
                        let Ok(target_id) = TargetID::try_from(node_id) else {
                            bail!(
                                "{node_id} is not a valid meta node ID\
                                (must be between 1 and 65535)"
                            );
                        };

                        db::target::insert_meta(tx, target_id, None)?;
                    }

                    (node_id, node_uid)
                };

                // Update the corresponding nic lists
                db::node_nic::replace(
                    tx,
                    node_uid,
                    msg.nics.iter().map(|e| ReplaceNic {
                        nic_type: e.nic_type,
                        addr: &e.addr,
                        name: std::str::from_utf8(&e.name).unwrap_or("INVALID_UTF8"),
                    }),
                )?;

                Ok((
                    node_uid,
                    node_id,
                    match node_type {
                        // In case this is a meta node, the requester expects info about the meta
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
                        MetaRoot::Normal(node_id, _) => node_id,
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

impl Handler for RemoveNode {
    type Response = RemoveNodeResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Self::Response {
        let res = ctx
            .db
            .op(move |tx| {
                let node_uid = db::node::get_uid(tx, self.node_id, self.node_type)?
                    .ok_or_else(|| TypedError::value_not_found("node ID", self.node_id))?;

                db::node::delete(tx, node_uid)?;

                Ok(())
            })
            .await;

        match res {
            Ok(_) => {
                log::info!("Removed {:?} node with ID {}", self.node_type, self.node_id,);

                notify_nodes(
                    ctx,
                    match self.node_type {
                        shared::types::NodeType::Meta => &[NodeType::Meta, NodeType::Client],
                        shared::types::NodeType::Storage => {
                            &[NodeType::Meta, NodeType::Storage, NodeType::Client]
                        }
                        _ => &[],
                    },
                    &RemoveNode {
                        ack_id: "".into(),
                        ..self
                    },
                )
                .await;

                RemoveNodeResp {
                    result: OpsErr::SUCCESS,
                }
            }
            Err(err) => {
                log_error_chain!(
                    err,
                    "Removing {:?} node with ID {} failed",
                    self.node_type,
                    self.node_id
                );

                RemoveNodeResp {
                    result: OpsErr::INTERNAL,
                }
            }
        }
    }
}

impl Handler for RemoveNodeResp {
    type Response = ();

    async fn handle(self, _ctx: &Context, req: &mut impl Request) -> Self::Response {
        // response from server nodes to the RemoveNode notification
        log::debug!("Ignoring RemoveNodeResp msg from {:?}", req.addr());
    }
}
