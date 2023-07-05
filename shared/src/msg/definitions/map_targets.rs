use super::*;

/// Maps targets to owning nodes
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct MapTargets {
    #[bee_serde(as = Map<false, _, _>)]
    pub target_ids: HashMap<TargetID, StoragePoolID>,
    #[bee_serde(as = Int<u32>)]
    pub node_id: NodeID,
    pub ack_id: AckID,
}

impl Msg for MapTargets {
    const ID: MsgID = MsgID(1023);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct MapTargetsResp {
    /// Maps a target ID to the mapping result
    #[bee_serde(as = Map<false, _, _>)]
    pub results: HashMap<TargetID, OpsErr>,
}

impl Msg for MapTargetsResp {
    const ID: MsgID = MsgID(1024);
}
