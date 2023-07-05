use super::*;

/// Sets usage info for a target.
///
/// Actually used for storage AND meta targets, despite the name.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetStorageTargetInfo {
    #[bee_serde(as = Int<i32>)]
    pub node_type: NodeTypeServer,
    #[bee_serde(as = Seq<false, _>)]
    pub info: Vec<TargetInfo>,
}

impl Msg for SetStorageTargetInfo {
    const ID: MsgID = MsgID(2099);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetStorageTargetInfoResp {
    pub result: OpsErr,
}

impl Msg for SetStorageTargetInfoResp {
    const ID: MsgID = MsgID(2100);
}
