use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetTargetConsistencyStates {
    #[bee_serde(as = Int<i32>)]
    pub node_type: NodeTypeServer,
    #[bee_serde(as = Seq<true, _>)]
    pub target_ids: Vec<TargetID>,
    #[bee_serde(as = Seq<true, _>)]
    pub states: Vec<TargetConsistencyState>,
    pub ack_id: AckID,
    #[bee_serde(as = BoolAsInt<u8>)]
    pub set_online: bool,
}

impl Msg for SetTargetConsistencyStates {
    const ID: MsgID = MsgID(1055);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetTargetConsistencyStatesResp {
    pub result: OpsErr,
}

impl Msg for SetTargetConsistencyStatesResp {
    const ID: MsgID = MsgID(1056);
}
