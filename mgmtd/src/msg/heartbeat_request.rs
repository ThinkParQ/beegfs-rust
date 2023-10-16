use super::*;
use shared::msg::heartbeat::{Heartbeat, HeartbeatRequest, Nic};
use shared::types::{NodeType, MGMTD_ID};

pub(super) async fn handle(
    _msg: HeartbeatRequest,
    ctx: &Context,
    _req: &impl Request,
) -> Heartbeat {
    Heartbeat {
        instance_version: 0,
        nic_list_version: 0,
        node_type: NodeType::Management,
        node_alias: "Management".into(),
        ack_id: "".into(),
        node_num_id: MGMTD_ID,
        root_num_id: 0,
        is_root_mirrored: 0,
        port: ctx.info.config.beegfs_port,
        port_tcp_unused: ctx.info.config.beegfs_port,
        nic_list: ctx
            .info
            .network_addrs
            .iter()
            .map(|e| Nic {
                addr: e.addr,
                name: e.name.clone().into_bytes(),
                nic_type: shared::types::NicType::Ethernet,
            })
            .collect(),
    }
}
