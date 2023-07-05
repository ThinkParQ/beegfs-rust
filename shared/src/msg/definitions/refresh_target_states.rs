use super::*;

/// Indicates a node to fetch the fresh target states list (sent via UDP).
///
/// Nodes then request the newest info via [GetTargetStates]. No idea why the info is not just
/// sent with this message.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RefreshTargetStates {
    pub ack_id: AckID,
}

impl Msg for RefreshTargetStates {
    const ID: MsgID = MsgID(1051);
}
