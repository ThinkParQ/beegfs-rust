use super::*;

/// Fetch default quota settings for the given storage pool
///
/// Used by old ctl only
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

/// Fetch quota info for the given type and list or range of IDs.
///
/// Used by the ctl to query management and by the managment to query the nodes
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GetQuotaInfo {
    pub query_type: QuotaQueryType,
    pub id_type: QuotaIDType,
    pub id_range_start: QuotaID,
    pub id_range_end: QuotaID,
    pub id_list: Vec<QuotaID>,
    pub transfer_method: GetQuotaInfoTransferMethod,
    /// If "one request per target" is chosen as transfer method, the target ID goes in here.
    ///
    /// It is the only mode ctl uses, and the only mode we use to send requests to the node.
    pub target_id: TargetID,
    /// If targets shall be combined by pool in one response message, the pool ID goes in here.
    ///
    /// Completely unused.
    pub pool_id: StoragePoolID,
}

impl GetQuotaInfo {
    pub fn with_group_ids(
        mut group_ids: HashSet<QuotaID>,
        target_id: TargetID,
        pool_id: StoragePoolID,
    ) -> Self {
        Self {
            query_type: QuotaQueryType::List,
            id_type: QuotaIDType::Group,
            id_range_start: 0,
            id_range_end: 0,
            id_list: group_ids.drain().collect(),
            transfer_method: GetQuotaInfoTransferMethod::AllTargetsOneRequestPerTarget,
            target_id,
            pool_id,
        }
    }

    pub fn with_user_ids(
        mut user_ids: HashSet<QuotaID>,
        target_id: TargetID,
        pool_id: StoragePoolID,
    ) -> Self {
        Self {
            query_type: QuotaQueryType::List,
            id_type: QuotaIDType::User,
            id_range_start: 0,
            id_range_end: 0,
            id_list: user_ids.drain().collect(),
            transfer_method: GetQuotaInfoTransferMethod::AllTargetsOneRequestPerTarget,
            target_id,
            pool_id,
        }
    }
}

impl Msg for GetQuotaInfo {
    const ID: MsgID = 2097;
}

// Custom BeeSerde impl because (de-)serialization actions depend on msg data
impl BeeSerde for GetQuotaInfo {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.i32(self.query_type.into())?;
        ser.i32(self.id_type.into())?;

        if self.query_type == QuotaQueryType::Range {
            ser.u32(self.id_range_start)?;
            ser.u32(self.id_range_end)?;
        } else if self.query_type == QuotaQueryType::List {
            ser.seq(self.id_list.iter(), true, |ser, e| ser.u32(*e))?;
        } else if self.query_type == QuotaQueryType::Single {
            ser.u32(self.id_range_start)?;
        }

        ser.u32(self.transfer_method.into())?;
        self.target_id.serialize(ser)?;
        self.pool_id.serialize(ser)?;
        Ok(())
    }

    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        let query_type: QuotaQueryType = des.i32()?.try_into()?;

        Ok(Self {
            query_type,
            id_type: des.i32()?.try_into()?,
            id_range_start: match query_type {
                QuotaQueryType::Range | QuotaQueryType::Single => des.u32()?,
                _ => 0,
            },
            id_range_end: match query_type {
                QuotaQueryType::Range => des.u32()?,
                _ => 0,
            },
            id_list: match query_type {
                QuotaQueryType::List => des.seq(true, |des| des.u32())?,
                _ => vec![],
            },
            transfer_method: des.u32()?.try_into()?,
            target_id: TargetID::deserialize(des)?,
            pool_id: StoragePoolID::deserialize(des)?,
        })
    }
}

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

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetQuotaInfoResp {
    #[bee_serde(as = Int<u32>)]
    pub quota_inode_support: QuotaInodeSupport,
    #[bee_serde(as = Seq<false, _>)]
    pub quota_entry: Vec<QuotaEntry>,
}

impl Msg for GetQuotaInfoResp {
    const ID: MsgID = 2098;
}

/// Sets exceeded quota information on server nodes.
///
/// Also used as list entries in [RequestExceededQuota]
///
/// Used by self
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
    const ID: MsgID = 2077;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetExceededQuotaResp {
    pub result: OpsErr,
}

impl Msg for SetExceededQuotaResp {
    const ID: MsgID = 2078;
}

/// Fetches user / group IDs which exceed the quota limits.
///
/// Used by meta, storage
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
    const ID: MsgID = 2079;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RequestExceededQuotaResp {
    pub inner: SetExceededQuota,
    pub result: OpsErr,
}

impl Msg for RequestExceededQuotaResp {
    const ID: MsgID = 2080;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GetQuotaInfoTransferMethod {
    #[default]
    AllTargetsOneRequest = 0,
    AllTargetsOneRequestPerTarget = 1,
    SingleTarget = 2,
}

impl_enum_to_int!(GetQuotaInfoTransferMethod,
    AllTargetsOneRequest => 0,
    AllTargetsOneRequestPerTarget => 1,
    SingleTarget => 2
);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum QuotaInodeSupport {
    #[default]
    Unknown,
    AllBlockDevices,
    SomeBlockDevices,
    NoBlockDevices,
}

impl_enum_to_int!(QuotaInodeSupport,
    Unknown => 0,
    AllBlockDevices => 1,
    SomeBlockDevices => 2,
    NoBlockDevices => 3
);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum QuotaQueryType {
    #[default]
    None,
    Single,
    Range,
    List,
    All,
}

impl_enum_to_int!(QuotaQueryType,
    None => 0,
    Single => 1,
    Range => 2,
    List => 3,
    All => 4
);

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct QuotaEntry {
    pub space: u64,
    pub inodes: u64,
    pub id: QuotaID,
    #[bee_serde(as = Int<i32>)]
    pub id_type: QuotaIDType,
    pub valid: u8,
}
