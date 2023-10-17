use super::*;

/// Adds a new storage pool and moves the specified entities to that pool.
///
/// Used by old ctl only
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct AddStoragePool {
    pub pool_id: StoragePoolID,
    #[bee_serde(as = CStr<0>)]
    pub alias: Vec<u8>,
    #[bee_serde(as = Seq<true, _>)]
    pub move_target_ids: Vec<TargetID>,
    #[bee_serde(as = Seq<true, _>)]
    pub move_buddy_group_ids: Vec<BuddyGroupID>,
}

impl Msg for AddStoragePool {
    const ID: MsgID = 1064;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct AddStoragePoolResp {
    pub result: OpsErr,
    /// The ID used for the new pool
    pub pool_id: StoragePoolID,
}

impl Msg for AddStoragePoolResp {
    const ID: MsgID = 1065;
}
