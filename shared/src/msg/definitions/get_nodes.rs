use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetNodes {
    #[bee_serde(as = Int<u32>)]
    pub node_type: NodeType,
}

impl Msg for GetNodes {
    const ID: MsgID = MsgID(1017);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetNodesResp {
    #[bee_serde(as = Seq<false, _>)]
    pub nodes: Vec<Node>,
    // this can be a NodeNumID or a BuddyGroupID
    pub root_num_id: u32,
    #[bee_serde(as = BoolAsInt<u8>)]
    pub is_root_mirrored: bool,
}

impl Msg for GetNodesResp {
    const ID: MsgID = MsgID(1018);
}