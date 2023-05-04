use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetNodeCapacityPools {
    #[bee_serde(as = Int<i32>)]
    pub query_type: CapacityPoolQueryType,
}

impl Msg for GetNodeCapacityPools {
    const ID: MsgID = MsgID(1021);
}

/// The u16 ID is interpreted differently from BeeGFS, depending on the request.
/// If it requests buddy groups, it is a buddy group ID, if it is storage targets it is a
/// TargetNumID and if it is meta targets it is a NodeNumID.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GetNodeCapacityPoolsResp {
    pub pools: HashMap<StoragePoolID, Vec<Vec<u16>>>,
}

impl Msg for GetNodeCapacityPoolsResp {
    const ID: MsgID = MsgID(1022);
}

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