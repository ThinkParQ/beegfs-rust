use super::*;

/// Tells the existence of a node
///
/// Only used by the client after opening a connection.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct PeerInfo {
    #[bee_serde(as = Int<u32>)]
    pub node_type: NodeType,
    #[bee_serde(as = Int<u32>)]
    pub node_id: NodeID,
}

impl Msg for PeerInfo {
    const ID: MsgID = 4011;
}
