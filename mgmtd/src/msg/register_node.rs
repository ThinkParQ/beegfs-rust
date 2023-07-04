use super::*;
use db::entity::EntityType;
use db::misc::MetaRoot;

pub(super) async fn handle(
    msg: msg::RegisterNode,
    ci: impl ComponentInteractor,
    _rcc: &impl RequestConnectionController,
) -> msg::RegisterNodeResp {
    let node_id = update(msg, ci).await;

    msg::RegisterNodeResp {
        node_num_id: node_id,
    }
}

/// Processes incoming node information. Registeres new nodes if config allows it
pub(super) async fn update(msg: msg::RegisterNode, ci: impl ComponentInteractor) -> NodeID {
    let msg2 = msg.clone();
    let registration_enable = ci.get_config().registration_enable;

    match ci
        .db_op(move |tx| {
            let mut node_id = msg.node_id;
            let mut node_uid = db::node::get_uid(tx, msg.node_id, msg.node_type)?;
            let is_new_node = msg.node_id == NodeID::ZERO || node_uid.is_none();

            if is_new_node {
                // If the node is new, do additional checks and insert the prerequisites

                // TODO overhaul the config system and get this directly from the DB
                if !registration_enable {
                    return Err(DbError::other("Registration of new nodes is not allowed"));
                }

                // Check alias doesnt exist yet
                if db::entity::get_uid(tx, &msg.node_alias)?.is_some() {
                    return Err(DbError::value_exists("Alias", &msg.node_alias));
                };

                // Services send a 0 value when they want the new node to be assigned an ID
                // automatically
                node_id = if node_id == NodeID::ZERO {
                    db::misc::find_new_id(
                        tx,
                        &format!("{}_nodes", msg.node_type.as_sql_str()),
                        "node_id",
                        1..=0xFFFF,
                    )?
                    .into()
                } else {
                    node_id
                };

                // Insert new entity and node entry
                node_uid = Some(db::entity::insert(tx, EntityType::Node, &msg.node_alias)?);
                db::node::insert(tx, node_id, node_uid.unwrap(), msg.node_type, msg.port)?;

                // if this is a meta node, auto-add a corresponding meta target after the node. This
                // is required because currently the rest of BeeGFS doesn't know
                // about meta targets and expects exactly one meta target per meta
                // node (with the same ID)
                if msg.node_type == NodeType::Meta {
                    db::target::insert_meta(
                        tx,
                        u16::from(node_id).into(),
                        &format!("{}_target", msg.node_alias).into(),
                    )?;
                }
            } else {
                // Existing node, update attached information
                db::entity::update_alias(tx, node_uid.unwrap(), &msg.node_alias)?;
                db::node::update(tx, node_uid.unwrap(), msg.port)?;
            }

            // Update the corresponding nic lists
            db::node_nic::replace(tx, node_uid.unwrap(), &msg.nics)?;

            Ok((
                node_id,
                match msg.node_type {
                    // In case this is a meta node, the requestor expects info about the meta root
                    NodeType::Meta => db::misc::get_meta_root(tx)?,
                    _ => MetaRoot::Unknown,
                },
            ))
        })
        .await
    {
        Ok((node_id, meta_root)) => {
            log::info!(
                "Processed {} node info from with ID {} (Requested: {})",
                msg2.node_type,
                node_id,
                msg2.node_id,
            );

            // notify all nodes
            ci.notify_nodes(&msg::Heartbeat {
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
                    MetaRoot::Unknown | MetaRoot::Normal(_, _) => false,
                    MetaRoot::Mirrored(_) => true,
                },
                port: msg.port,
                port_tcp_unused: msg.port,
                nic_list: msg2.nics,
            })
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
