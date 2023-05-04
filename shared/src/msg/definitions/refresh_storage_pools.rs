use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RefreshStoragePools {
    pub ack_id: AckID,
}

impl Msg for RefreshStoragePools {
    const ID: MsgID = MsgID(1070);
}
