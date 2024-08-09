use super::*;

/// Fetch quota info for the given type and list or range of IDs.
///
/// Used by the ctl to query management and by the management to query the nodes
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GetQuotaInfo {
    pub query_type: QuotaQueryType,
    pub id_type: QuotaIdType,
    pub id_range_start: QuotaId,
    pub id_range_end: QuotaId,
    pub id_list: Vec<QuotaId>,
    pub transfer_method: GetQuotaInfoTransferMethod,
    /// If "one request per target" is chosen as transfer method, the target ID goes in here.
    ///
    /// It is the only mode ctl uses, and the only mode we use to send requests to the node.
    pub target_id: TargetId,
    /// If targets shall be combined by pool in one response message, the pool ID goes in here.
    ///
    /// Completely unused.
    pub pool_id: PoolId,
}

impl GetQuotaInfo {
    pub fn with_group_ids(
        mut group_ids: HashSet<QuotaId>,
        target_id: TargetId,
        pool_id: PoolId,
    ) -> Self {
        Self {
            query_type: QuotaQueryType::List,
            id_type: QuotaIdType::Group,
            id_range_start: 0,
            id_range_end: 0,
            id_list: group_ids.drain().collect(),
            transfer_method: GetQuotaInfoTransferMethod::AllTargetsOneRequestPerTarget,
            target_id,
            pool_id,
        }
    }

    pub fn with_user_ids(
        mut user_ids: HashSet<QuotaId>,
        target_id: TargetId,
        pool_id: PoolId,
    ) -> Self {
        Self {
            query_type: QuotaQueryType::List,
            id_type: QuotaIdType::User,
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
    const ID: MsgId = 2097;
}

// Custom BeeSerde impl because (de-)serialization actions depend on msg data
impl Serializable for GetQuotaInfo {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.i32(self.query_type.into_bee_serde())?;
        ser.i32(self.id_type.into_bee_serde())?;

        if self.query_type == QuotaQueryType::Range {
            ser.u32(self.id_range_start)?;
            ser.u32(self.id_range_end)?;
        } else if self.query_type == QuotaQueryType::List {
            ser.seq(self.id_list.iter(), true, |ser, e| ser.u32(*e))?;
        } else if self.query_type == QuotaQueryType::Single {
            ser.u32(self.id_range_start)?;
        }

        ser.u32(self.transfer_method.into_bee_serde())?;
        self.target_id.serialize(ser)?;
        self.pool_id.serialize(ser)?;
        Ok(())
    }
}

impl Deserializable for GetQuotaInfo {
    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        let query_type: QuotaQueryType = BeeSerdeConversion::try_from_bee_serde(des.i32()?)?;

        Ok(Self {
            query_type,
            id_type: BeeSerdeConversion::try_from_bee_serde(des.i32()?)?,
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
            transfer_method: BeeSerdeConversion::try_from_bee_serde(des.u32()?)?,
            target_id: TargetId::deserialize(des)?,
            pool_id: PoolId::deserialize(des)?,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetQuotaInfoResp {
    #[bee_serde(as = Int<u32>)]
    pub quota_inode_support: QuotaInodeSupport,
    #[bee_serde(as = Seq<false, _>)]
    pub quota_entry: Vec<QuotaEntry>,
}

impl Msg for GetQuotaInfoResp {
    const ID: MsgId = 2098;
}

/// Sets exceeded quota information on server nodes.
///
/// Also used as list entries in [RequestExceededQuota]
///
/// Used by self
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetExceededQuota {
    pub pool_id: PoolId,
    #[bee_serde(as = Int<i32>)]
    pub id_type: QuotaIdType,
    #[bee_serde(as = Int<i32>)]
    pub quota_type: QuotaType,
    #[bee_serde(as = Seq<true, _>)]
    pub exceeded_quota_ids: Vec<QuotaId>,
}

impl Msg for SetExceededQuota {
    const ID: MsgId = 2077;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetExceededQuotaResp {
    pub result: OpsErr,
}

impl Msg for SetExceededQuotaResp {
    const ID: MsgId = 2078;
}

/// Fetches user / group IDs which exceed the quota limits.
///
/// Used by meta, storage
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RequestExceededQuota {
    #[bee_serde(as = Int<i32>)]
    pub id_type: QuotaIdType,
    #[bee_serde(as = Int<i32>)]
    pub quota_type: QuotaType,
    pub pool_id: PoolId,
    pub target_id: TargetId,
}

impl Msg for RequestExceededQuota {
    const ID: MsgId = 2079;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RequestExceededQuotaResp {
    pub inner: SetExceededQuota,
    pub result: OpsErr,
}

impl Msg for RequestExceededQuotaResp {
    const ID: MsgId = 2080;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GetQuotaInfoTransferMethod {
    #[default]
    AllTargetsOneRequest = 0,
    AllTargetsOneRequestPerTarget = 1,
    SingleTarget = 2,
}

impl_enum_bee_msg_traits!(GetQuotaInfoTransferMethod,
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

impl_enum_bee_msg_traits!(QuotaInodeSupport,
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

impl_enum_bee_msg_traits!(QuotaQueryType,
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
    pub id: QuotaId,
    #[bee_serde(as = Int<i32>)]
    pub id_type: QuotaIdType,
    pub valid: u8,
}
