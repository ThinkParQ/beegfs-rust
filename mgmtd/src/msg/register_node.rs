use super::*;
use db::misc::MetaRoot;
use shared::config::RegistrationEnable;

pub(super) async fn handle(
    msg: msg::RegisterNode,
    rcc: impl RequestConnectionController,
    ci: impl ComponentInteractor,
) -> Result<()> {
    let node_id = process(msg, ci).await;

    rcc.respond(&msg::RegisterNodeResp {
        node_num_id: node_id,
    })
    .await
}

/// Processes incoming node information. Registeres new nodes if config allows it
pub(super) async fn process(msg: msg::RegisterNode, ci: impl ComponentInteractor) -> NodeID {
    match async {
        let msg = msg.clone();
        let enable_registration = ci.get_config::<RegistrationEnable>();

        let (node_id, meta_root) = ci
            .execute_db(move |tx| {
                Ok((
                    db::nodes::set(
                        tx,
                        enable_registration,
                        msg.node_num_id,
                        msg.node_type,
                        msg.node_alias,
                        msg.port,
                        msg.nic_list,
                    )?,
                    match msg.node_type {
                        NodeType::Meta => db::misc::get_meta_root(tx)?,
                        _ => MetaRoot::Unknown,
                    },
                ))
            })
            .await?;

        Ok((node_id, meta_root)) as Result<_>
    }
    .await
    {
        Ok((node_id, meta_root)) => {
            log::info!(
                "Processed {} node info from with ID {} (Requested: {})",
                msg.node_type,
                node_id,
                msg.node_num_id,
            );

            // notify all nodes
            ci.notify_nodes(&msg::Heartbeat {
                instance_version: 0,
                nic_list_version: 0,
                node_type: msg.node_type,
                node_alias: msg.node_alias,
                ack_id: "".into(),
                node_num_id: node_id,
                root_num_id: match meta_root {
                    MetaRoot::Unknown => 0,
                    MetaRoot::Normal(_, node_id, _) => node_id.into(),
                    MetaRoot::Mirrored(buddy_group_id) => buddy_group_id.into(),
                },
                is_root_mirrored: match meta_root {
                    MetaRoot::Unknown | MetaRoot::Normal(_, _, _) => false,
                    MetaRoot::Mirrored(_) => true,
                },
                port: msg.port,
                port_tcp_unused: msg.port,
                nic_list: msg.nic_list,
            })
            .await;

            node_id
        }

        Err(err) => {
            log::error!(
                "Processing {} node info for ID {} failed:\n{:?}",
                msg.node_type,
                msg.node_num_id,
                err
            );

            NodeID::ZERO
        }
    }
}
