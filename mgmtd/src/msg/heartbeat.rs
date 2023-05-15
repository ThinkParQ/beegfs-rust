use super::*;
use shared::msg::Ack;

pub(super) async fn handle(
    msg: msg::Heartbeat,
    rcc: impl RequestConnectionController,
    ci: impl ComponentInteractor,
) -> Result<()> {
    let _ = register_node::process(
        msg::RegisterNode {
            instance_version: msg.instance_version,
            nic_list_version: msg.nic_list_version,
            node_alias: msg.node_alias,
            nic_list: msg.nic_list,
            node_type: msg.node_type,
            node_num_id: msg.node_num_id,
            root_num_id: msg.root_num_id,
            is_root_mirrored: msg.is_root_mirrored,
            port: msg.port,
            port_tcp_unused: msg.port_tcp_unused,
        },
        ci,
    )
    .await;

    rcc.respond(&Ack { ack_id: msg.ack_id }).await
}
