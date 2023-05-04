use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct UnmapStorageTarget {
    pub target_id: TargetID,
}

impl Msg for UnmapStorageTarget {
    const ID: MsgID = MsgID(1027);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct UnmapStorageTargetResp {
    pub result: OpsErr,
}

impl Msg for UnmapStorageTargetResp {
    const ID: MsgID = MsgID(1028);
}
