use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetMirrorBuddyGroup {
    #[bee_serde(as = Int<u32>)]
    pub node_type: NodeTypeServer,
    pub primary_target: TargetID,
    pub secondary_target: TargetID,
    pub buddy_group_id: BuddyGroupID,
    #[bee_serde(as = BoolAsInt<u8>)]
    pub allow_update: bool,
    pub ack_id: AckID,
}

impl Msg for SetMirrorBuddyGroup {
    const ID: MsgID = MsgID(1045);
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SetMirrorBuddyGroupResp {
    pub result: OpsErr,
    pub buddy_group_id: BuddyGroupID,
}

impl Msg for SetMirrorBuddyGroupResp {
    const ID: MsgID = MsgID(1046);
}

impl BeeSerde for SetMirrorBuddyGroupResp {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        self.result.serialize(ser)?;
        self.buddy_group_id.serialize(ser)?;
        ser.zeroes(2)?;
        Ok(())
    }

    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        let r = Self {
            result: OpsErr::deserialize(des)?,
            buddy_group_id: BuddyGroupID::deserialize(des)?,
        };
        des.skip(2)?;
        Ok(r)
    }
}
