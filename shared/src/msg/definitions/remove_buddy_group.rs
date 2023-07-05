use super::*;

/// Removes a buddy group from the system.
///
/// Currently only supported for storage buddy groups, despite the field `node_type`.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RemoveBuddyGroup {
    #[bee_serde(as = Int<i32>)]
    pub node_type: NodeTypeServer,
    pub buddy_group_id: BuddyGroupID,
    #[bee_serde(as = BoolAsInt<u8>)]
    pub check_only: bool,
    #[bee_serde(as = BoolAsInt<u8>)]
    pub force: bool,
}

impl Msg for RemoveBuddyGroup {
    const ID: MsgID = MsgID(1060);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RemoveBuddyGroupResp {
    pub result: OpsErr,
}

impl Msg for RemoveBuddyGroupResp {
    const ID: MsgID = MsgID(1061);
}
