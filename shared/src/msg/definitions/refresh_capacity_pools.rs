use super::*;

/// Indicates anodes to fetch fresh capacity info from management (sent via UDP).
///
/// Nodes then request the newest info via [GetNodeCapacityPools]. No idea why the info is not just
/// sent with this message.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RefreshCapacityPools {
    pub ack_id: AckID,
}

impl Msg for RefreshCapacityPools {
    const ID: MsgID = MsgID(1035);
}
