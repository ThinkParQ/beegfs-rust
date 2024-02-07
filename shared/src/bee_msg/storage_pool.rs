use super::*;

/// Adds a new storage pool and moves the specified entities to that pool.
///
/// Used by old ctl only
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct AddStoragePool {
    pub pool_id: StoragePoolID,
    #[bee_serde(as = CStr<0>)]
    pub alias: Vec<u8>,
    #[bee_serde(as = Seq<true, _>)]
    pub move_target_ids: Vec<TargetID>,
    #[bee_serde(as = Seq<true, _>)]
    pub move_buddy_group_ids: Vec<BuddyGroupID>,
}

impl Msg for AddStoragePool {
    const ID: MsgID = 1064;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct AddStoragePoolResp {
    pub result: OpsErr,
    /// The ID used for the new pool
    pub pool_id: StoragePoolID,
}

impl Msg for AddStoragePoolResp {
    const ID: MsgID = 1065;
}

/// Modifies an existing storage pool and adds/removes targets fromto/from this pool
///
/// Targets removed shall be put into the default pool.
///
/// Used by old ctl only
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModifyStoragePool {
    pub pool_id: StoragePoolID,
    pub alias: Option<Vec<u8>>,
    pub add_target_ids: Vec<TargetID>,
    pub remove_target_ids: Vec<TargetID>,
    pub add_buddy_group_ids: Vec<BuddyGroupID>,
    pub remove_buddy_group_ids: Vec<BuddyGroupID>,
}

impl ModifyStoragePool {
    const HAS_DESC: u16 = 1;
    const HAS_ADD_TARGETS: u16 = 2;
    const HAS_REMOVE_TARGETS: u16 = 4;
    const HAS_ADD_GROUPS: u16 = 8;
    const HAS_REMOVE_GROUPS: u16 = 16;
}

impl Msg for ModifyStoragePool {
    const ID: MsgID = 1068;
}

/// Custom BeeSerde impl because actions depend on flags set in the msg header
impl Deserializable for ModifyStoragePool {
    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        let flags = des.msg_feature_flags;

        Ok(Self {
            pool_id: StoragePoolID::deserialize(des)?,
            alias: if flags & Self::HAS_DESC != 0 {
                Some(des.cstr(0)?)
            } else {
                None
            },
            add_target_ids: if flags & Self::HAS_ADD_TARGETS != 0 {
                des.seq(true, |des| TargetID::deserialize(des))?
            } else {
                vec![]
            },
            remove_target_ids: if flags & Self::HAS_REMOVE_TARGETS != 0 {
                des.seq(true, |des| TargetID::deserialize(des))?
            } else {
                vec![]
            },
            add_buddy_group_ids: if flags & Self::HAS_ADD_GROUPS != 0 {
                des.seq(true, |des| BuddyGroupID::deserialize(des))?
            } else {
                vec![]
            },
            remove_buddy_group_ids: if flags & Self::HAS_REMOVE_GROUPS != 0 {
                des.seq(true, |des| BuddyGroupID::deserialize(des))?
            } else {
                vec![]
            },
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct ModifyStoragePoolResp {
    pub result: OpsErr,
}

impl Msg for ModifyStoragePoolResp {
    const ID: MsgID = 1069;
}

/// Removes a storage pool from the system
///
/// Used by old ctl only
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RemoveStoragePool {
    pub pool_id: StoragePoolID,
}

impl Msg for RemoveStoragePool {
    const ID: MsgID = 1071;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RemoveStoragePoolResp {
    pub result: OpsErr,
}

impl Msg for RemoveStoragePoolResp {
    const ID: MsgID = 1072;
}

/// Fetches all storage pools.
///
/// Used by at least old ctl, meta, storage
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetStoragePools {}

impl Msg for GetStoragePools {
    const ID: MsgID = 1066;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetStoragePoolsResp {
    #[bee_serde(as = Seq<true, StoragePool>)]
    pub pools: Vec<StoragePool>,
}

impl Msg for GetStoragePoolsResp {
    const ID: MsgID = 1067;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct StoragePool {
    pub id: StoragePoolID,
    #[bee_serde(as = CStr<0>)]
    pub alias: Vec<u8>,
    #[bee_serde(as = Seq<true, _>)]
    pub targets: Vec<TargetID>,
    #[bee_serde(as = Seq<true, _>)]
    pub buddy_groups: Vec<BuddyGroupID>,
    pub target_cap_pools: TargetCapacityPools,
    pub buddy_cap_pools: BuddyGroupCapacityPools,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TargetCapacityPools {
    pub pools: Vec<Vec<TargetID>>,
    pub grouped_target_pools: Vec<HashMap<NodeID, Vec<TargetID>>>,
    pub target_map: HashMap<TargetID, NodeID>,
}

impl Serializable for TargetCapacityPools {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.seq(self.pools.iter(), true, |ser, e| {
            ser.seq(e.iter(), true, |ser, e| e.serialize(ser))
        })?;
        ser.seq(self.grouped_target_pools.iter(), true, |ser, e| {
            ser.map(
                e.iter(),
                false,
                |ser, k| ser.u32((*k).into()),
                |ser, v| ser.seq(v.iter(), true, |ser, e| e.serialize(ser)),
            )
        })?;
        ser.map(
            self.target_map.iter(),
            false,
            |ser, k| k.serialize(ser),
            |ser, v| ser.u32((*v).into()),
        )?;
        Ok(())
    }
}

impl Deserializable for TargetCapacityPools {
    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        Ok(Self {
            pools: des.seq(true, |des| des.seq(true, |des| TargetID::deserialize(des)))?,
            grouped_target_pools: des.seq(true, |des| {
                des.map(
                    false,
                    |des| Ok(des.u32()?.try_into()?),
                    |des| des.seq(true, |des| TargetID::deserialize(des)),
                )
            })?,
            target_map: des.map(
                false,
                |des| TargetID::deserialize(des),
                |des| Ok(des.u32()?.try_into()?),
            )?,
        })
    }
}

/// BeeGFS actually deserializes this into a NodeCapacityPools object
/// which contains TargetNumIDs but in reality are buddy group ids.
/// Since TargetNumIDs are represented as u16, it's very important
/// to only change them together if this is ever changed!
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct BuddyGroupCapacityPools {
    pub pools: Vec<Vec<BuddyGroupID>>,
}

impl Serializable for BuddyGroupCapacityPools {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.seq(self.pools.iter(), true, |ser, e| {
            ser.seq(e.iter(), true, |ser, e| e.serialize(ser))
        })
    }
}

impl Deserializable for BuddyGroupCapacityPools {
    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        Ok(Self {
            pools: des.seq(true, |des| {
                des.seq(true, |des| BuddyGroupID::deserialize(des))
            })?,
        })
    }
}

/// Indicates a node to fetch the fresh storage pool list (sent via UDP)
///
/// Nodes then request the newest info via [GetStoragePools]. No idea why the info is not just
/// sent with this message.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RefreshStoragePools {
    #[bee_serde(as = CStr<0>)]
    pub ack_id: Vec<u8>,
}

impl Msg for RefreshStoragePools {
    const ID: MsgID = 1070;
}
