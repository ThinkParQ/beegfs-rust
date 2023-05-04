use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetMirrorBuddyGroups {
    #[bee_serde(as = Int<u32>)]
    pub node_type: NodeTypeServer,
}

impl Msg for GetMirrorBuddyGroups {
    const ID: MsgID = MsgID(1047);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetMirrorBuddyGroupsResp {
    #[bee_serde(as = Seq<true, _>)]
    pub buddy_groups: Vec<BuddyGroupID>,
    #[bee_serde(as = Seq<true, _>)]
    pub primary_targets: Vec<TargetID>,
    #[bee_serde(as = Seq<true, _>)]
    pub secondary_targets: Vec<TargetID>,
}

impl Msg for GetMirrorBuddyGroupsResp {
    const ID: MsgID = MsgID(1048);
}