use super::*;

/// Unmap a storage target.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct UnmapTarget {
    pub target_id: TargetID,
}

impl Msg for UnmapTarget {
    const ID: MsgID = MsgID(1027);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct UnmapTargetResp {
    pub result: OpsErr,
}

impl Msg for UnmapTargetResp {
    const ID: MsgID = MsgID(1028);
}
