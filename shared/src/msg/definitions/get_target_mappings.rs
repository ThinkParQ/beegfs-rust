use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetTargetMappings {}

impl Msg for GetTargetMappings {
    const ID: MsgID = MsgID(1025);
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GetTargetMappingsResp {
    pub mapping: HashMap<TargetID, NodeID>,
}

impl Msg for GetTargetMappingsResp {
    const ID: MsgID = MsgID(1026);
}

impl BeeSerde for GetTargetMappingsResp {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.map(
            self.mapping.iter(),
            false,
            |ser, k| k.serialize(ser),
            |ser, v| ser.u32((*v).into()),
        )
    }

    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        Ok(Self {
            mapping: des.map(
                false,
                |des| TargetID::deserialize(des),
                |des| Ok(des.u32()?.try_into()?),
            )?,
        })
    }
}
