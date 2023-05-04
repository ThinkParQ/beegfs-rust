use super::*;

pub(super) async fn handle(
    _msg: msg::HeartbeatRequest,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    chn.respond(&msg::Heartbeat {
        instance_version: 0,
        nic_list_version: 0,
        node_type: NodeType::Management,
        node_alias: "Management".into(),
        ack_id: "".into(),
        node_num_id: NodeID::MGMTD,
        root_num_id: 0,
        is_root_mirrored: false,
        port: hnd.get_static_info().static_config.port,
        port_tcp_unused: hnd.get_static_info().static_config.port,
        nic_list: hnd.get_static_info().network_interfaces.to_vec(),
    })
    .await
}
