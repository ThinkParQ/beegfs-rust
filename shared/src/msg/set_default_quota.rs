use super::*;

/// Sets default quota limits per storage pool
///
/// Used by old ctl only
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetDefaultQuota {
    pub pool_id: StoragePoolID,
    pub space: u64,
    pub inodes: u64,
    #[bee_serde(as = Int<i32>)]
    pub id_type: QuotaIDType,
}

impl Msg for SetDefaultQuota {
    const ID: MsgID = 2111;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetDefaultQuotaResp {
    pub result: i32,
}

impl Msg for SetDefaultQuotaResp {
    const ID: MsgID = 2112;
}
