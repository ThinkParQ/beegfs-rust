use super::*;
use db::misc::MetaRoot;
use shared::config::RegistrationEnable;

pub(super) async fn handle(
    msg: msg::RegisterNode,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    match async {
        if !hnd.get_config::<RegistrationEnable>() {
            bail!("Registration of new nodes is disabled");
        }

        let msg = msg.clone();

        let node_id = hnd
            .execute_db(move |tx| {
                db::nodes::set(
                    tx,
                    true,
                    msg.node_num_id,
                    msg.node_type,
                    msg.node_alias,
                    msg.port,
                    msg.nic_list,
                )
            })
            .await?;

        let meta_root = match msg.node_type {
            NodeType::Meta => hnd.execute_db(db::misc::get_meta_root).await?,
            _ => MetaRoot::Unknown,
        };

        Ok((node_id, meta_root)) as Result<_>
    }
    .await
    {
        Ok((node_id, meta_root)) => {
            log::info!(
                "Registered {} node with ID {} (Requested: {})",
                msg.node_type,
                node_id,
                msg.node_num_id,
            );

            // notify all nodes
            hnd.notify_nodes(&msg::Heartbeat {
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

            chn.respond(&msg::RegisterNodeResp {
                node_num_id: node_id,
            })
            .await
        }
        Err(err) => {
            log::error!(
                "Registering {} node with requested ID {} failed:\n{:?}",
                msg.node_type,
                msg.node_num_id,
                err
            );

            chn.respond(&msg::RegisterNodeResp {
                node_num_id: NodeID::ZERO,
            })
            .await
        }
    }
}
