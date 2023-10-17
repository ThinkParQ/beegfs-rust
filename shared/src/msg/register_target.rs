use super::*;

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
