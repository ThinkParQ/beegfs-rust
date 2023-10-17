use super::*;

/// Set consistency states for a list of targets of the given node type.
///
/// Some nodes receive this via UDP, therefore the msg has an AckID field. Similar to
/// [SetTargetConsistencyStates].
///
/// Used by meta, storage, fsck, old ctl
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct ChangeTargetConsistencyStates {
    #[bee_serde(as = Int<i32>)]
    pub node_type: NodeType,
    #[bee_serde(as = Seq<true, _>)]
    pub target_ids: Vec<TargetID>,
    #[bee_serde(as = Seq<true, _>)]
    pub old_states: Vec<TargetConsistencyState>,
    #[bee_serde(as = Seq<true, _>)]
    pub new_states: Vec<TargetConsistencyState>,
    #[bee_serde(as = CStr<0>)]
    pub ack_id: Vec<u8>,
}

impl Msg for ChangeTargetConsistencyStates {
    const ID: MsgID = 1057;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct ChangeTargetConsistencyStatesResp {
    pub result: OpsErr,
}

impl Msg for ChangeTargetConsistencyStatesResp {
    const ID: MsgID = 1058;
}
