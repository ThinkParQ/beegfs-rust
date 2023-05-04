use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct Ack {
    pub ack_id: AckID,
}

impl Msg for Ack {
    const ID: MsgID = MsgID(4003);
}