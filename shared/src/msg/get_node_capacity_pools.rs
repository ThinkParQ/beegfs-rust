use super::*;

/// Fetches node capacity pools of the given type for all targets / groups.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetNodeCapacityPools {
    #[bee_serde(as = Int<i32>)]
    pub query_type: CapacityPoolQueryType,
}

impl Msg for GetNodeCapacityPools {
    const ID: MsgID = 1021;
}

/// Response containing node capacity bool mapping
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GetNodeCapacityPoolsResp {
    /// Target or group IDs grouped by storage pool and cap pool.
    ///
    /// The outer Vec has index 0, 1, 2 containing lists with IDs belonging to that pool.
    pub pools: HashMap<StoragePoolID, Vec<Vec<u16>>>,
}

impl Msg for GetNodeCapacityPoolsResp {
    const ID: MsgID = 1022;
}

// Custom BeeSerde impl because nested sequences / maps are not supported by the macro
impl BeeSerde for GetNodeCapacityPoolsResp {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.map(
            self.pools.iter(),
            false,
            |ser, k| k.serialize(ser),
            |ser, v| {
                ser.seq(v.iter(), true, |ser, e| {
                    ser.seq(e.iter(), true, |ser, g| ser.u16(*g))
                })
            },
        )
    }

    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        Ok(Self {
            pools: des.map(
                false,
                |des| StoragePoolID::deserialize(des),
                |des| des.seq(true, |des| des.seq(true, |des| des.u16())),
            )?,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CapacityPoolQueryType {
    #[default]
    Meta,
    Storage,
    MetaMirrored,
    StorageMirrored,
}

impl_enum_to_int!(CapacityPoolQueryType, Meta => 0, Storage => 1, MetaMirrored => 2, StorageMirrored => 3);
