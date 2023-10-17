use super::*;
pub use crate::msg::get_quota_info::QuotaEntry;

/// Set the quota limits for a given storage pool.
///
/// Used by old ctl only
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetQuota {
    pub pool_id: StoragePoolID,
    #[bee_serde(as = Seq<false, _>)]
    pub quota_entry: Vec<QuotaEntry>,
}

impl Msg for SetQuota {
    const ID: MsgID = 2075;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetQuotaResp {
    pub result: i32,
}

impl Msg for SetQuotaResp {
    const ID: MsgID = 2076;
}
