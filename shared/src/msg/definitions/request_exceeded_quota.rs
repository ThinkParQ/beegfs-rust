use super::*;

/// Fetches user / group IDs which exceed the quota limits.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RequestExceededQuota {
    #[bee_serde(as = Int<i32>)]
    pub id_type: QuotaIDType,
    #[bee_serde(as = Int<i32>)]
    pub quota_type: QuotaType,
    pub pool_id: StoragePoolID,
    pub target_id: TargetID,
}

impl Msg for RequestExceededQuota {
    const ID: MsgID = MsgID(2079);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RequestExceededQuotaResp {
    pub inner: SetExceededQuota,
    pub result: OpsErr,
}

impl Msg for RequestExceededQuotaResp {
    const ID: MsgID = MsgID(2080);
}
