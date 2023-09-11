use super::*;

pub(super) async fn handle(
    _msg: msg::HeartbeatRequest,
    ctx: &impl AppContext,
    _req: &impl Request,
) -> msg::Heartbeat {
    msg::Heartbeat {
        instance_version: 0,
        nic_list_version: 0,
        node_type: NodeType::Management,
        node_alias: "Management".into(),
        ack_id: "".into(),
        node_num_id: NodeID::MGMTD,
        root_num_id: 0,
        is_root_mirrored: 0,
        port: ctx.runtime_info().config.port,
        port_tcp_unused: ctx.runtime_info().config.port,
        nic_list: ctx.runtime_info().network_interfaces.to_vec(),
    }
}
