use super::*;
pub use crate::msg::get_nodes::Nic;

/// Requests a heartbeat from a node.
///
/// Usually sent by UDP.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct HeartbeatRequest {}

impl Msg for HeartbeatRequest {
    const ID: MsgID = 1019;
}

/// Updates a node with the given information.
///
/// Similar to [RegisterNode]
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct Heartbeat {
    /// Unused
    pub instance_version: u64,
    /// Unused
    pub nic_list_version: u64,
    #[bee_serde(as = Int<i32>)]
    pub node_type: NodeType,
    #[bee_serde(as = CStr<0>)]
    pub node_alias: Vec<u8>,
    #[bee_serde(as = CStr<4>)]
    pub ack_id: Vec<u8>,
    #[bee_serde(as = Int<u32>)]
    pub node_num_id: NodeID,
    // The root info is only relevant when sent from meta nodes. There it must contain the meta
    // root nodes ID, but on other nodes it is just irrelevant.
    // Can be a Node ID or a BuddyGroup ID
    pub root_num_id: u32,
    pub is_root_mirrored: u8,
    pub port: Port,
    /// This is transmitted from other nodes but we decided to just use one port for TCP and UDP in
    /// the future
    pub port_tcp_unused: Port,
    #[bee_serde(as = Seq<false, _>)]
    pub nic_list: Vec<Nic>,
}

impl Msg for Heartbeat {
    const ID: MsgID = 1020;
}
