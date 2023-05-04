use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetChannelDirect {
    #[bee_serde(as = BoolAsInt<i32>)]
    pub is_direct: bool,
}

impl Msg for SetChannelDirect {
    const ID: MsgID = MsgID(4001);
}