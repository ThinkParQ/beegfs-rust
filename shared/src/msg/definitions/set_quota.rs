use super::*;

/// Used by the server nodes to set quota usage information on the management
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetQuota {
    pub pool_id: StoragePoolID,
    #[bee_serde(as = Seq<false, _>)]
    pub quota_entry: Vec<QuotaEntry>,
}

impl Msg for SetQuota {
    const ID: MsgID = MsgID(2075);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetQuotaResp {
    #[bee_serde(as = BoolAsInt<i32>)]
    pub result: bool,
}

impl Msg for SetQuotaResp {
    const ID: MsgID = MsgID(2076);
}
