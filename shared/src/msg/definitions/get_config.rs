use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetConfig {}

impl Msg for GetConfig {
    const ID: MsgID = MsgID(10002);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct GetAllConfigResp {
    #[bee_serde(as = Map<false, _, _>)]
    pub entries: config::ConfigMap,
}

impl Msg for GetAllConfigResp {
    const ID: MsgID = MsgID(10003);
}

