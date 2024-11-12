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
/// Used by old ctl, meta, storage, client
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetStatesAndBuddyGroups {
    #[bee_serde(as = Int<i32>)]
    pub node_type: NodeType,
    pub requested_by_client_id: NodeId,
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

/// Overrides the last buddy communication timestamp on the primary node.
/// Resynchronizes all changes after the specified timestamp to the secondary node.
///
/// Used by old ctl, storage and self
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetLastBuddyCommOverride {
    pub target_id: TargetId,
    pub timestamp: i64,
    pub abort_resync: u8,
}

impl Msg for SetLastBuddyCommOverride {
    const ID: MsgId = 2095;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetLastBuddyCommOverrideResp {
    pub result: OpsErr,
}

impl Msg for SetLastBuddyCommOverrideResp {
    const ID: MsgId = 2096;
}

/// Fetch resynchronization statistics from storage node
///
/// Used by old ctl, new ctl, storage and self
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetStorageResyncStats {
    pub target_id: TargetId,
}

impl Msg for GetStorageResyncStats {
    const ID: MsgId = 2093;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetStorageResyncStatsResp {
    #[bee_serde(as = Int<i32>)]
    pub state: BuddyResyncJobState,
    pub start_time: i64,
    pub end_time: i64,
    pub discovered_files: u64,
    pub discovered_dirs: u64,
    pub matched_files: u64,
    pub matched_dirs: u64,
    pub synced_files: u64,
    pub synced_dirs: u64,
    pub error_files: u64,
    pub error_dirs: u64,
}

impl Msg for GetStorageResyncStatsResp {
    const ID: MsgId = 2094;
}

/// Fetch resynchronization statistics for meta node
///
/// Used by old ctl, meta, new ctl and self
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetMetaResyncStats {
    pub target_id: TargetId,
}

impl Msg for GetMetaResyncStats {
    const ID: MsgId = 2117;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetMetaResyncStatsResp {
    #[bee_serde(as = Int<i32>)]
    pub state: BuddyResyncJobState,
    pub start_time: i64,
    pub end_time: i64,
    pub discovered_dirs: u64,
    pub gather_errors: u64,
    pub synced_dirs: u64,
    pub synced_files: u64,
    pub error_dirs: u64,
    pub error_files: u64,
    pub sessions_to_sync: u64,
    pub synced_sessions: u64,
    pub session_sync_errors: u8,
    pub mod_objects_synced: u64,
    pub mod_sync_errors: u64,
}

impl Msg for GetMetaResyncStatsResp {
    const ID: MsgId = 2118;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BuddyResyncJobState {
    #[default]
    NotStarted,
    Running,
    Success,
    Interrupted,
    Failure,
    Errors,
}

impl_enum_bee_msg_traits!(BuddyResyncJobState,
    NotStarted => 0,
    Running => 1,
    Success => 2,
    Interrupted => 3,
    Failure => 4,
    Errors => 5
);
