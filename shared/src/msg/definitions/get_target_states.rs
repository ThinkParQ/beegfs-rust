use super::*;

/// Fetches a mapping target ID to target states
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetTargetStates {
    #[bee_serde(as = Int<i32>)]
    pub node_type: NodeTypeServer,
}

impl Msg for GetTargetStates {
    const ID: MsgID = MsgID(1049);
}

/// Contains three Vecs containing the requested mapping
///
/// The elements in the same position in the Vecs / sequences belong together.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetTargetStatesResp {
    #[bee_serde(as = Seq<true, _>)]
    pub targets: Vec<TargetID>,
    #[bee_serde(as = Seq<true, _>)]
    pub reachability_states: Vec<TargetReachabilityState>,
    #[bee_serde(as = Seq<true, _>)]
    pub consistency_states: Vec<TargetConsistencyState>,
}

impl Msg for GetTargetStatesResp {
    const ID: MsgID = MsgID(1050);
}
