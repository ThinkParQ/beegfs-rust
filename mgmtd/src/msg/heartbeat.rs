use super::*;

pub(super) async fn handle(msg: msg::Heartbeat, ctx: &Context, _req: &impl Request) -> msg::Ack {
    let _ = register_node::update(
        msg::RegisterNode {
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

    msg::Ack { ack_id: msg.ack_id }
}
