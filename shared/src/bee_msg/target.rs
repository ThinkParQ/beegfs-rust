use super::*;

/// Fetches a mapping target ID to its owner node ID
///
/// Used by old ctl, fsck, meta, storage
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetTargetMappings {}

impl Msg for GetTargetMappings {
    const ID: MsgID = 1025;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GetTargetMappingsResp {
    pub mapping: HashMap<TargetID, NodeID>,
}

impl Msg for GetTargetMappingsResp {
    const ID: MsgID = 1026;
}

impl Serializable for GetTargetMappingsResp {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.map(
            self.mapping.iter(),
            false,
            |ser, k| k.serialize(ser),
            |ser, v| ser.u32(*v),
        )
    }
}

/// Fetches a mapping target ID to target states
///
/// Used by old ctl, fsck, storage
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetTargetStates {
    #[bee_serde(as = Int<i32>)]
    pub node_type: NodeType,
}

impl Msg for GetTargetStates {
    const ID: MsgID = 1049;
}

/// Contains three Vecs containing the requested mapping
///
/// The elements in the same position in the Vecs / sequences belong together.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetTargetStatesResp {
    #[bee_serde(as = Seq<true, _>)]
    pub targets: Vec<TargetID>,
    #[bee_serde(as = Seq<true, _>)]
    pub reachability_states: Vec<TargetReachabilityState>,
    #[bee_serde(as = Seq<true, _>)]
    pub consistency_states: Vec<TargetConsistencyState>,
}

impl Msg for GetTargetStatesResp {
    const ID: MsgID = 1050;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TargetReachabilityState {
    #[default]
    Online,
    ProbablyOffline,
    Offline,
}

impl_enum_to_int!(TargetReachabilityState,
    Online => 0,
    ProbablyOffline => 1,
    Offline => 2
);

impl Serializable for TargetReachabilityState {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.u8((*self).into())
    }
}

impl Deserializable for TargetReachabilityState {
    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        des.u8()?.try_into()
    }
}

/// Registers a new storage target.
///
/// The new target is supposed to be mapped after using [MapTargets].
///
/// Used by storage
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RegisterTarget {
    #[bee_serde(as = CStr<0>)]
    pub alias: Vec<u8>,
    pub target_id: TargetID,
}

impl Msg for RegisterTarget {
    const ID: MsgID = 1041;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RegisterTargetResp {
    pub id: TargetID,
}

impl Msg for RegisterTargetResp {
    const ID: MsgID = 1042;
}

/// Maps targets to owning nodes
///
/// Used by old ctl, storage
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct MapTargets {
    #[bee_serde(as = Map<false, _, _>)]
    pub target_ids: HashMap<TargetID, StoragePoolID>,
    #[bee_serde(as = Int<u32>)]
    pub node_id: NodeID,
    #[bee_serde(as = CStr<0>)]
    pub ack_id: Vec<u8>,
}

impl Msg for MapTargets {
    const ID: MsgID = 1023;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct MapTargetsResp {
    /// Maps a target ID to the mapping result
    #[bee_serde(as = Map<false, _, _>)]
    pub results: HashMap<TargetID, OpsErr>,
}

impl Msg for MapTargetsResp {
    const ID: MsgID = 1024;
}

/// Unmap a storage target.
///
/// Used by old ctl only
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct UnmapTarget {
    pub target_id: TargetID,
}

impl Msg for UnmapTarget {
    const ID: MsgID = 1027;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct UnmapTargetResp {
    pub result: OpsErr,
}

impl Msg for UnmapTargetResp {
    const ID: MsgID = 1028;
}

/// Set consistency states for a list of targets of the given node type.
///
/// Some nodes receive this via UDP, therefore the msg has an AckID field. Similar to
/// [SetTargetConsistencyStates].
///
/// Used by meta, storage, fsck, old ctl
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct ChangeTargetConsistencyStates {
    #[bee_serde(as = Int<i32>)]
    pub node_type: NodeType,
    #[bee_serde(as = Seq<true, _>)]
    pub target_ids: Vec<TargetID>,
    #[bee_serde(as = Seq<true, _>)]
    pub old_states: Vec<TargetConsistencyState>,
    #[bee_serde(as = Seq<true, _>)]
    pub new_states: Vec<TargetConsistencyState>,
    #[bee_serde(as = CStr<4>)]
    pub ack_id: Vec<u8>,
}

impl Msg for ChangeTargetConsistencyStates {
    const ID: MsgID = 1057;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct ChangeTargetConsistencyStatesResp {
    pub result: OpsErr,
}

impl Msg for ChangeTargetConsistencyStatesResp {
    const ID: MsgID = 1058;
}

/// Set consistency states for a list of targets of the given node type.
///
/// Some nodes receive this via UDP, therefore the msg has an AckID field. Similar to
/// [ChangeTargetConsistencyStates].
///
/// Used by meta, storage
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetTargetConsistencyStates {
    #[bee_serde(as = Int<i32>)]
    pub node_type: NodeType,
    #[bee_serde(as = Seq<true, _>)]
    pub target_ids: Vec<TargetID>,
    #[bee_serde(as = Seq<true, _>)]
    pub states: Vec<TargetConsistencyState>,
    #[bee_serde(as = CStr<4>)]
    pub ack_id: Vec<u8>,
    pub set_online: u8,
}

impl Msg for SetTargetConsistencyStates {
    const ID: MsgID = 1055;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetTargetConsistencyStatesResp {
    pub result: OpsErr,
}

impl Msg for SetTargetConsistencyStatesResp {
    const ID: MsgID = 1056;
}

/// Sets usage info for a target.
///
/// Actually used for storage AND meta targets, despite the name.
///
/// Used by meta, storage
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetStorageTargetInfo {
    #[bee_serde(as = Int<i32>)]
    pub node_type: NodeType,
    #[bee_serde(as = Seq<false, _>)]
    pub info: Vec<TargetInfo>,
}

impl Msg for SetStorageTargetInfo {
    const ID: MsgID = 2099;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetStorageTargetInfoResp {
    pub result: OpsErr,
}

impl Msg for SetStorageTargetInfoResp {
    const ID: MsgID = 2100;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct TargetInfo {
    pub target_id: TargetID,
    #[bee_serde(as = CStr<4>)]
    pub path: Vec<u8>,
    #[bee_serde(as = Int<i64>)]
    pub total_space: u64,
    #[bee_serde(as = Int<i64>)]
    pub free_space: u64,
    #[bee_serde(as = Int<i64>)]
    pub total_inodes: u64,
    #[bee_serde(as = Int<i64>)]
    pub free_inodes: u64,
    pub consistency_state: TargetConsistencyState,
}

/// Indicates a node to fetch the fresh target states list (sent via UDP).
///
/// Nodes then request the newest info via [GetTargetStates]. No idea why the info is not just
/// sent with this message.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RefreshTargetStates {
    #[bee_serde(as = CStr<0>)]
    pub ack_id: Vec<u8>,
}

impl Msg for RefreshTargetStates {
    const ID: MsgID = 1051;
}
