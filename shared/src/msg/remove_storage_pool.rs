use super::*;

/// Removes a storage pool from the system
///
/// Used by old ctl only
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RemoveStoragePool {
    pub pool_id: StoragePoolID,
}

impl Msg for RemoveStoragePool {
    const ID: MsgID = 1071;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RemoveStoragePoolResp {
    pub result: OpsErr,
}

impl Msg for RemoveStoragePoolResp {
    const ID: MsgID = 1072;
}
