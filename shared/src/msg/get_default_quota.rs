use super::*;

/// Fetch default quota settings for the given storage pool
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetDefaultQuota {
    pub pool_id: StoragePoolID,
}

impl Msg for GetDefaultQuota {
    const ID: MsgID = 2109;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetDefaultQuotaResp {
    pub limits: QuotaDefaultLimits,
}

impl Msg for GetDefaultQuotaResp {
    const ID: MsgID = 2110;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct QuotaDefaultLimits {
    pub user_inode_limit: u64,
    pub user_space_limit: u64,
    pub group_inode_limit: u64,
    pub group_space_limit: u64,
}
