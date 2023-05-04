use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RefreshTargetStates {
    pub ack_id: AckID,
}

impl Msg for RefreshTargetStates {
    const ID: MsgID = MsgID(1051);
}
