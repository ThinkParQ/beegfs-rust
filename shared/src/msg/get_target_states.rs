use super::*;

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

impl BeeSerde for TargetReachabilityState {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.u8((*self).into())
    }

    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        des.u8()?.try_into()
    }
}
