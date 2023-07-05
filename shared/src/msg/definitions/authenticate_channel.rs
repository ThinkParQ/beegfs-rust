use super::*;

/// Authenticate the communication channel (e.g. the TCP connection on which this message comes in).
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct AuthenticateChannel {
    pub auth_secret: AuthenticationSecret,
}

impl Msg for AuthenticateChannel {
    const ID: MsgID = MsgID(4007);
}
