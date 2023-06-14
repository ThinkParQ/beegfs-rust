use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetConfig {}

impl Msg for GetConfig {
    const ID: MsgID = MsgID(10002);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetConfigResp {
    #[bee_serde(as = Map<false, _, _>)]
    pub entries: config::ConfigMap,
}

impl Msg for GetConfigResp {
    const ID: MsgID = MsgID(10003);
}
