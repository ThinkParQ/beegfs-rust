use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct HeartbeatRequest {}

impl Msg for HeartbeatRequest {
    const ID: MsgID = MsgID(1019);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct Heartbeat {
    pub instance_version: u64,
    pub nic_list_version: u64,
    #[bee_serde(as = Int<i32>)]
    pub node_type: NodeType,
    #[bee_serde(as = CStr<0>)]
    pub node_alias: EntityAlias,
    #[bee_serde(as = CStr<4>)]
    pub ack_id: AckID,
    #[bee_serde(as = Int<u32>)]
    pub node_num_id: NodeID,
    // The root info is only relevant when sent from meta nodes. There it must contain the meta
    // root nodes ID, but on other nodes it is just irrelevant and can be set to whatever.
    // Can be Node ID or BuddyGroup ID
    pub root_num_id: u32,
    #[bee_serde(as = BoolAsInt<u8>)]
    pub is_root_mirrored: bool,
    pub port: Port,
    pub port_tcp_unused: Port,
    #[bee_serde(as = Seq<false, _>)]
    pub nic_list: Vec<Nic>,
}

impl Msg for Heartbeat {
    const ID: MsgID = MsgID(1020);
}
