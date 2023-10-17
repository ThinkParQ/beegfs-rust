use super::*;

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

impl BeeSerde for TargetCapacityPools {
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

impl BeeSerde for BuddyGroupCapacityPools {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.seq(self.pools.iter(), true, |ser, e| {
            ser.seq(e.iter(), true, |ser, e| e.serialize(ser))
        })
    }

    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        Ok(Self {
            pools: des.seq(true, |des| {
                des.seq(true, |des| BuddyGroupID::deserialize(des))
            })?,
        })
    }
}
