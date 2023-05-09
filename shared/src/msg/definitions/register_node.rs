use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RegisterNode {
    pub instance_version: u64,
    pub nic_list_version: u64,
    #[bee_serde(as = CStr<0>)]
    pub node_alias: EntityAlias,
    #[bee_serde(as = Seq<false, _>)]
    pub nic_list: Vec<Nic>,
    #[bee_serde(as = Int<i32>)]
    pub node_type: NodeType,
    #[bee_serde(as = Int<u32>)]
    pub node_num_id: NodeID,
    pub root_num_id: u32,
    #[bee_serde(as = BoolAsInt<u8>)]
    pub is_root_mirrored: bool,
    pub port: Port,
    pub port_tcp_unused: Port,
}

impl Msg for RegisterNode {
    const ID: MsgID = MsgID(1039);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RegisterNodeResp {
    #[bee_serde(as = Int<u32>)]
    pub node_num_id: NodeID,
}

impl Msg for RegisterNodeResp {
    const ID: MsgID = MsgID(1040);
}
