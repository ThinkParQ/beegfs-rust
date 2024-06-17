use super::*;

/// Fetches all storage pools.
///
/// Used by at least old ctl, meta, storage
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetStoragePools {}

impl Msg for GetStoragePools {
    const ID: MsgId = 1066;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetStoragePoolsResp {
    #[bee_serde(as = Seq<true, StoragePool>)]
    pub pools: Vec<StoragePool>,
}

impl Msg for GetStoragePoolsResp {
    const ID: MsgId = 1067;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct StoragePool {
    pub id: PoolId,
    #[bee_serde(as = CStr<0>)]
    pub alias: Vec<u8>,
    #[bee_serde(as = Seq<true, _>)]
    pub targets: Vec<TargetId>,
    #[bee_serde(as = Seq<true, _>)]
    pub buddy_groups: Vec<BuddyGroupId>,
    pub target_cap_pools: TargetCapacityPools,
    pub buddy_cap_pools: BuddyGroupCapacityPools,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TargetCapacityPools {
    pub pools: Vec<Vec<TargetId>>,
    pub grouped_target_pools: Vec<HashMap<NodeId, Vec<TargetId>>>,
    pub target_map: HashMap<TargetId, NodeId>,
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
                |ser, k| ser.u32(*k),
                |ser, v| ser.seq(v.iter(), true, |ser, e| e.serialize(ser)),
            )
        })?;
        ser.map(
            self.target_map.iter(),
            false,
            |ser, k| k.serialize(ser),
            |ser, v| ser.u32(*v),
        )?;
        Ok(())
    }
}

impl Deserializable for TargetCapacityPools {
    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        Ok(Self {
            pools: des.seq(true, |des| des.seq(true, |des| TargetId::deserialize(des)))?,
            grouped_target_pools: des.seq(true, |des| {
                des.map(
                    false,
                    |des| des.u32(),
                    |des| des.seq(true, |des| TargetId::deserialize(des)),
                )
            })?,
            target_map: des.map(false, |des| TargetId::deserialize(des), |des| des.u32())?,
        })
    }
}

/// BeeGFS actually deserializes this into a NodeCapacityPools object
/// which contains TargetNumIDs but in reality are buddy group ids.
/// Since TargetNumIDs are represented as u16, it's very important
/// to only change them together if this is ever changed!
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct BuddyGroupCapacityPools {
    pub pools: Vec<Vec<BuddyGroupId>>,
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
                des.seq(true, |des| BuddyGroupId::deserialize(des))
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
    const ID: MsgId = 1070;
}
