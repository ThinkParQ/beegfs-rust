use super::*;

/// Adds a new buddy group or notifies the nodes via UDP that there is a new buddy group
///
/// Used by old ctl, self
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetMirrorBuddyGroup {
    #[bee_serde(as = Int<u32>)]
    pub node_type: NodeType,
    pub primary_target_id: TargetID,
    pub secondary_target_id: TargetID,
    pub buddy_group_id: BuddyGroupID,
    /// This probably shall allow a group to be updated
    pub allow_update: u8,
    #[bee_serde(as = CStr<0>)]
    pub ack_id: Vec<u8>,
}

impl Msg for SetMirrorBuddyGroup {
    const ID: MsgID = 1045;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SetMirrorBuddyGroupResp {
    pub result: OpsErr,
    pub buddy_group_id: BuddyGroupID,
}

impl Msg for SetMirrorBuddyGroupResp {
    const ID: MsgID = 1046;
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
