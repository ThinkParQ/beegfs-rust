use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetDefaultQuota {
    pub pool_id: StoragePoolID,
}

impl Msg for GetDefaultQuota {
    const ID: MsgID = MsgID(2109);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetDefaultQuotaResp {
    pub limits: QuotaDefaultLimits,
}

impl Msg for GetDefaultQuotaResp {
    const ID: MsgID = MsgID(2110);
}
