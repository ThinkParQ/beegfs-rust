use super::*;

/// Expected response when a UDP message has been received.
///
/// Does actually nothing on BeeGFS nodes (except for maybe printing an error after some timeout).
/// Incoming Acks can just be ignored.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct Ack {
    #[bee_serde(as = CStr<0>)]
    pub ack_id: Vec<u8>,
}

impl Msg for Ack {
    const ID: MsgID = 4003;
}
