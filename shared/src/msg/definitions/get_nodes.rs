use super::*;

/// Fetch all nodes of the given type
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetNodes {
    #[bee_serde(as = Int<u32>)]
    pub node_type: NodeType,
}

impl Msg for GetNodes {
    const ID: MsgID = 1017;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetNodesResp {
    #[bee_serde(as = Seq<false, _>)]
    pub nodes: Vec<Node>,
    /// If the requested node type was Meta, then this contains the target / buddy group ID which
    /// owns the root inode.
    pub root_num_id: u32,
    /// Determines wether root_num_id is a target or buddy group ID
    pub is_root_mirrored: u8,
}

impl Msg for GetNodesResp {
    const ID: MsgID = 1018;
}
