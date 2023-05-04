use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RemoveNode {
    #[bee_serde(as = Int<i16>)]
    pub node_type: NodeType,
    #[bee_serde(as = Int<u32>)]
    pub num_id: NodeID,
    pub ack_id: AckID,
}

impl Msg for RemoveNode {
    const ID: MsgID = MsgID(1013);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RemoveNodeResp {
    pub result: OpsErr,
}

impl Msg for RemoveNodeResp {
    const ID: MsgID = MsgID(1014);
}
