use super::*;

/// Expected response when a UDP message has been received.
///
/// Does actually nothing on BeeGFS nodes (except for maybe printing an error after some timeout).
/// Incoming Acks can just be ignored.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct Ack {
    pub ack_id: AckID,
}

impl Msg for Ack {
    const ID: MsgID = MsgID(4003);
}
