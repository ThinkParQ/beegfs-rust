use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct PeerInfo {
    #[bee_serde(as = Int<u32>)]
    pub node_type: NodeType,
    #[bee_serde(as = Int<u32>)]
    pub node_id: NodeID,
}

impl Msg for PeerInfo {
    const ID: MsgID = MsgID(4011);
}
