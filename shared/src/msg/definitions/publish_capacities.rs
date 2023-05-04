use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct PublishCapacities {
    pub ack_id: AckID,
}

impl Msg for PublishCapacities {
    const ID: MsgID = MsgID(1059);
}
