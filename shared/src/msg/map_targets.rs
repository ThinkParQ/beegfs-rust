use super::*;

/// Maps targets to owning nodes
///
/// Used by old ctl, storage
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct MapTargets {
    #[bee_serde(as = Map<false, _, _>)]
    pub target_ids: HashMap<TargetID, StoragePoolID>,
    #[bee_serde(as = Int<u32>)]
    pub node_id: NodeID,
    #[bee_serde(as = CStr<0>)]
    pub ack_id: Vec<u8>,
}

impl Msg for MapTargets {
    const ID: MsgID = 1023;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct MapTargetsResp {
    /// Maps a target ID to the mapping result
    #[bee_serde(as = Map<false, _, _>)]
    pub results: HashMap<TargetID, OpsErr>,
}

impl Msg for MapTargetsResp {
    const ID: MsgID = 1024;
}
