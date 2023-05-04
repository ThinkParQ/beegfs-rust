use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct MapTargets {
    #[bee_serde(as = Map<false, _, _>)]
    pub targets: HashMap<TargetID, StoragePoolID>,
    #[bee_serde(as = Int<u32>)]
    pub node_num_id: NodeID,
    pub ack_id: AckID,
}

impl Msg for MapTargets {
    const ID: MsgID = MsgID(1023);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct MapTargetsResp {
    #[bee_serde(as = Map<false, _, _>)]
    pub results: HashMap<TargetID, OpsErr>,
}

impl Msg for MapTargetsResp {
    const ID: MsgID = MsgID(1024);
}
