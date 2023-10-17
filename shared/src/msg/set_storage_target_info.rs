use super::*;

/// Sets usage info for a target.
///
/// Actually used for storage AND meta targets, despite the name.
///
/// Used by meta, storage
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetStorageTargetInfo {
    #[bee_serde(as = Int<i32>)]
    pub node_type: NodeType,
    #[bee_serde(as = Seq<false, _>)]
    pub info: Vec<TargetInfo>,
}

impl Msg for SetStorageTargetInfo {
    const ID: MsgID = 2099;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetStorageTargetInfoResp {
    pub result: OpsErr,
}

impl Msg for SetStorageTargetInfoResp {
    const ID: MsgID = 2100;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct TargetInfo {
    pub target_id: TargetID,
    #[bee_serde(as = CStr<4>)]
    pub path: Vec<u8>,
    #[bee_serde(as = Int<i64>)]
    pub total_space: u64,
    #[bee_serde(as = Int<i64>)]
    pub free_space: u64,
    #[bee_serde(as = Int<i64>)]
    pub total_inodes: u64,
    #[bee_serde(as = Int<i64>)]
    pub free_inodes: u64,
    pub consistency_state: TargetConsistencyState,
}
