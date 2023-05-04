use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetStatesAndBuddyGroups {
    #[bee_serde(as = Int<u32>)]
    pub node_type: NodeTypeServer,
}

impl Msg for GetStatesAndBuddyGroups {
    const ID: MsgID = MsgID(1053);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetStatesAndBuddyGroupsResp {
    #[bee_serde(as = Map<false, _, _>)]
    pub groups: HashMap<BuddyGroupID, BuddyGroup>,
    #[bee_serde(as = Map<false, _, _>)]
    pub states: HashMap<TargetID, CombinedTargetState>,
}

impl Msg for GetStatesAndBuddyGroupsResp {
    const ID: MsgID = MsgID(1054);
}

