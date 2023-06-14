use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RegisterStorageTarget {
    #[bee_serde(as = CStr<0>)]
    pub alias: EntityAlias,
    pub target_id: TargetID,
}

impl Msg for RegisterStorageTarget {
    const ID: MsgID = MsgID(1041);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RegisterStorageTargetResp {
    pub id: TargetID,
}

impl Msg for RegisterStorageTargetResp {
    const ID: MsgID = MsgID(1042);
}
