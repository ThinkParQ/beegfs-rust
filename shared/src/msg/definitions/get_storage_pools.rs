use super::*;

/// Fetches all storage pools.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetStoragePools {}

impl Msg for GetStoragePools {
    const ID: MsgID = MsgID(1066);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetStoragePoolsResp {
    #[bee_serde(as = Seq<true, StoragePool>)]
    pub pools: Vec<StoragePool>,
}

impl Msg for GetStoragePoolsResp {
    const ID: MsgID = MsgID(1067);
}
