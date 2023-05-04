use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RefreshCapacityPools {
    pub ack_id: AckID,
}

impl Msg for RefreshCapacityPools {
    const ID: MsgID = MsgID(1035);
}
