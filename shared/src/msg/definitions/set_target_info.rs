use super::*;

// this actually matches to BeeGFS SetStorageTargetInfo, but in reality,
// it is also used for meta "targets". Therefore the different name.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetTargetInfo {
    #[bee_serde(as = Int<i32>)]
    pub node_type: NodeTypeServer,
    #[bee_serde(as = Seq<false, _>)]
    pub info: Vec<TargetInfo>,
}

impl Msg for SetTargetInfo {
    const ID: MsgID = MsgID(2099);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetTargetInfoResp {
    pub result: OpsErr,
}

impl Msg for SetTargetInfoResp {
    const ID: MsgID = MsgID(2100);
}
