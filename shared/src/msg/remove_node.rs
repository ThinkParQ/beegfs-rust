use super::*;

/// Remove a node from the system
///
/// Used by old ctl only
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RemoveNode {
    #[bee_serde(as = Int<i16>)]
    pub node_type: NodeType,
    #[bee_serde(as = Int<u32>)]
    pub node_id: NodeID,
    #[bee_serde(as = CStr<0>)]
    pub ack_id: Vec<u8>,
}

impl Msg for RemoveNode {
    const ID: MsgID = 1013;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RemoveNodeResp {
    pub result: OpsErr,
}

impl Msg for RemoveNodeResp {
    const ID: MsgID = 1014;
}
