use super::*;
use shared::msg::ack::Ack;
use shared::msg::heartbeat::Heartbeat;
use shared::msg::register_node::RegisterNode;

pub(super) async fn handle(msg: Heartbeat, ctx: &Context, _req: &impl Request) -> Ack {
    let _ = register_node::update(
        RegisterNode {
            instance_version: msg.instance_version,
            nic_list_version: msg.nic_list_version,
            node_alias: msg.node_alias,
            nics: msg.nic_list,
            node_type: msg.node_type,
            node_id: msg.node_num_id,
            root_num_id: msg.root_num_id,
            is_root_mirrored: msg.is_root_mirrored,
            port: msg.port,
            port_tcp_unused: msg.port_tcp_unused,
        },
        ctx,
    )
    .await;

    Ack { ack_id: msg.ack_id }
}
