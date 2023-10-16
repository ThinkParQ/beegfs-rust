use super::*;
pub use crate::msg::get_nodes::Nic;

/// Registers a new node with the given information.
///
/// Similar to [Heartbeat]
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RegisterNode {
    /// Unused
    pub instance_version: u64,
    /// Unused
    pub nic_list_version: u64,
    #[bee_serde(as = CStr<0>)]
    pub node_alias: Vec<u8>,
    #[bee_serde(as = Seq<false, _>)]
    pub nics: Vec<Nic>,
    #[bee_serde(as = Int<i32>)]
    pub node_type: NodeType,
    #[bee_serde(as = Int<u32>)]
    pub node_id: NodeID,
    pub root_num_id: u32,
    pub is_root_mirrored: u8,
    pub port: Port,
    /// This is transmitted from other nodes but we decided to just use one port for TCP and UDP in
    /// the future
    pub port_tcp_unused: Port,
}

impl Msg for RegisterNode {
    const ID: MsgID = 1039;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RegisterNodeResp {
    #[bee_serde(as = Int<u32>)]
    pub node_num_id: NodeID,
}

impl Msg for RegisterNodeResp {
    const ID: MsgID = 1040;
}
