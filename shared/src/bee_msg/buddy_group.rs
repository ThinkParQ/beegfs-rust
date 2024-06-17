use super::target::TargetReachabilityState;
use super::*;

/// Fetch buddy groups of the given node type
///
/// Used by old ctl, fsck, mon, storage
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetMirrorBuddyGroups {
    #[bee_serde(as = Int<u32>)]
    pub node_type: NodeType,
}

impl Msg for GetMirrorBuddyGroups {
    const ID: MsgId = 1047;
}

/// Response with requested buddy groups along with their assigned targets.
///
/// The elements in the same position in the Vecs / sequences belong together.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetMirrorBuddyGroupsResp {
    #[bee_serde(as = Seq<true, _>)]
    pub buddy_groups: Vec<BuddyGroupId>,
    #[bee_serde(as = Seq<true, _>)]
    pub primary_targets: Vec<TargetId>,
    #[bee_serde(as = Seq<true, _>)]
    pub secondary_targets: Vec<TargetId>,
}

impl Msg for GetMirrorBuddyGroupsResp {
    const ID: MsgId = 1048;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct CombinedTargetState {
    pub reachability: TargetReachabilityState,
    pub consistency: TargetConsistencyState,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct BuddyGroup {
    pub primary_target_id: TargetId,
    pub secondary_target_id: TargetId,
}

/// Fetches a buddy group ids with their assigned targets and target ids with their states
///
/// Used by old ctl, meta, storage
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetStatesAndBuddyGroups {
    #[bee_serde(as = Int<u32>)]
    pub node_type: NodeType,
}

impl Msg for GetStatesAndBuddyGroups {
    const ID: MsgId = 1053;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetStatesAndBuddyGroupsResp {
    #[bee_serde(as = Map<false, _, _>)]
    pub groups: HashMap<BuddyGroupId, BuddyGroup>,
    #[bee_serde(as = Map<false, _, _>)]
    pub states: HashMap<TargetId, CombinedTargetState>,
}

impl Msg for GetStatesAndBuddyGroupsResp {
    const ID: MsgId = 1054;
}

/// Removes a buddy group from the system.
///
/// Currently only supported for storage buddy groups, despite the field `node_type`.
///
/// Used by old ctl and self
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RemoveBuddyGroup {
    #[bee_serde(as = Int<i32>)]
    pub node_type: NodeType,
    pub group_id: BuddyGroupId,
    pub check_only: u8,
    pub force: u8,
}

impl Msg for RemoveBuddyGroup {
    const ID: MsgId = 1060;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RemoveBuddyGroupResp {
    pub result: OpsErr,
}

impl Msg for RemoveBuddyGroupResp {
    const ID: MsgId = 1061;
}

/// Enables a metadata mirrored system
///
/// Used by old ctl and self
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetMetadataMirroring {}

impl Msg for SetMetadataMirroring {
    const ID: MsgId = 2069;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetMetadataMirroringResp {
    pub result: OpsErr,
}

impl Msg for SetMetadataMirroringResp {
    const ID: MsgId = 2070;
}

/// Adds a new buddy group or notifies the nodes via UDP that there is a new buddy group
///
/// Used by old ctl, self
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetMirrorBuddyGroup {
    #[bee_serde(as = Int<u32>)]
    pub node_type: NodeType,
    pub primary_target_id: TargetId,
    pub secondary_target_id: TargetId,
    pub group_id: BuddyGroupId,
    /// This probably shall allow a group to be updated
    pub allow_update: u8,
    #[bee_serde(as = CStr<0>)]
    pub ack_id: Vec<u8>,
}

impl Msg for SetMirrorBuddyGroup {
    const ID: MsgId = 1045;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SetMirrorBuddyGroupResp {
    pub result: OpsErr,
    pub group_id: BuddyGroupId,
}

impl Msg for SetMirrorBuddyGroupResp {
    const ID: MsgId = 1046;
}

impl Serializable for SetMirrorBuddyGroupResp {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        self.result.serialize(ser)?;
        self.group_id.serialize(ser)?;
        ser.zeroes(2)?;
        Ok(())
    }
}

impl Deserializable for SetMirrorBuddyGroupResp {
    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        let r = Self {
            result: OpsErr::deserialize(des)?,
            group_id: BuddyGroupId::deserialize(des)?,
        };
        des.skip(2)?;
        Ok(r)
    }
}
