use super::get_target_states::TargetReachabilityState;
use super::*;

/// Fetches a buddy group ids with their assigned targets and target ids with their states
///
/// Used by old ctl, meta, storage
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetStatesAndBuddyGroups {
    #[bee_serde(as = Int<u32>)]
    pub node_type: NodeType,
}

impl Msg for GetStatesAndBuddyGroups {
    const ID: MsgID = 1053;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetStatesAndBuddyGroupsResp {
    #[bee_serde(as = Map<false, _, _>)]
    pub groups: HashMap<BuddyGroupID, BuddyGroup>,
    #[bee_serde(as = Map<false, _, _>)]
    pub states: HashMap<TargetID, CombinedTargetState>,
}

impl Msg for GetStatesAndBuddyGroupsResp {
    const ID: MsgID = 1054;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct CombinedTargetState {
    pub reachability: TargetReachabilityState,
    pub consistency: TargetConsistencyState,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct BuddyGroup {
    pub primary_target_id: TargetID,
    pub secondary_target_id: TargetID,
}
