use super::*;

/// Set consistency states for a list of targets of the given node type.
///
/// Some nodes receive this via UDP, therefore the msg has an AckID field. Similar to
/// [SetTargetConsistencyStates].
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct ChangeTargetConsistencyStates {
    #[bee_serde(as = Int<i32>)]
    pub node_type: NodeTypeServer,
    #[bee_serde(as = Seq<true, _>)]
    pub target_ids: Vec<TargetID>,
    #[bee_serde(as = Seq<true, _>)]
    pub old_states: Vec<TargetConsistencyState>,
    #[bee_serde(as = Seq<true, _>)]
    pub new_states: Vec<TargetConsistencyState>,
    pub ack_id: AckID,
}

impl Msg for ChangeTargetConsistencyStates {
    const ID: MsgID = MsgID(1057);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct ChangeTargetConsistencyStatesResp {
    pub result: OpsErr,
}

impl Msg for ChangeTargetConsistencyStatesResp {
    const ID: MsgID = MsgID(1058);
}
