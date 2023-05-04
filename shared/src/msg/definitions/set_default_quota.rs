use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetDefaultQuota {
    pub pool_id: StoragePoolID,
    pub space: Space,
    pub inodes: Inodes,
    #[bee_serde(as = Int<i32>)]
    pub id_type: QuotaIDType,
}

impl Msg for SetDefaultQuota {
    const ID: MsgID = MsgID(2111);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetDefaultQuotaResp {
    #[bee_serde(as = BoolAsInt<i32>)]
    pub result: bool,
}

impl Msg for SetDefaultQuotaResp {
    const ID: MsgID = MsgID(2112);
}
