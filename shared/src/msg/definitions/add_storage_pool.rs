use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct AddStoragePool {
    pub id: StoragePoolID,
    #[bee_serde(as = CStr<0>)]
    pub alias: EntityAlias,
    #[bee_serde(as = Seq<true, _>)]
    pub move_target_ids: Vec<TargetID>,
    #[bee_serde(as = Seq<true, _>)]
    pub move_buddy_group_ids: Vec<BuddyGroupID>,
}

impl Msg for AddStoragePool {
    const ID: MsgID = MsgID(1064);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct AddStoragePoolResp {
    pub result: OpsErr,
    pub pool_id: StoragePoolID,
}

impl Msg for AddStoragePoolResp {
    const ID: MsgID = MsgID(1065);
}
