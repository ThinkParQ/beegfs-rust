use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetConfig {
    #[bee_serde(as = Map<false, String, String>)]
    pub entries: config::ConfigMap,
}

impl Msg for SetConfig {
    const ID: MsgID = MsgID(10000);
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetConfigResp {
    // TODO replace with something else?
    pub result: OpsErr,
}

impl Msg for SetConfigResp {
    const ID: MsgID = MsgID(10001);
}
