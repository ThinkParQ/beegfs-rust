use super::*;

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
