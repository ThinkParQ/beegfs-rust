use super::*;

/// Sets the type of the worker that handles this connection channel.
///
/// Unused/ignored in the Rust code.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct SetChannelDirect {
    pub is_direct: i32,
}

impl Msg for SetChannelDirect {
    const ID: MsgID = MsgID(4001);
}
