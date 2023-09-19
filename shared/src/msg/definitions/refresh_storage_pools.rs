use super::*;

/// Indicates a node to fetch the fresh storage pool list (sent via UDP)
///
/// Nodes then request the newest info via [GetStoragePools]. No idea why the info is not just
/// sent with this message.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct RefreshStoragePools {
    #[bee_serde(as = CStr<0>)]
    pub ack_id: Vec<u8>,
}

impl Msg for RefreshStoragePools {
    const ID: MsgID = 1070;
}
