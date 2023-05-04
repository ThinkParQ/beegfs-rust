use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetExceededQuota {
    pub pool_id: StoragePoolID,
    #[bee_serde(as = Int<i32>)]
    pub id_type: QuotaIDType,
    #[bee_serde(as = Int<i32>)]
    pub quota_type: QuotaType,
    #[bee_serde(as = Seq<true, _>)]
    pub exceeded_quota_ids: Vec<QuotaID>,
}

impl Msg for SetExceededQuota {
    const ID: MsgID = MsgID(2077);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetExceededQuotaResp {
    pub result: OpsErr,
}

impl Msg for SetExceededQuotaResp {
    const ID: MsgID = MsgID(2078);
}
