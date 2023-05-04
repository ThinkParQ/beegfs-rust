use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct AuthenticateChannel {
    pub auth_secret: AuthenticationSecret,
}

impl Msg for AuthenticateChannel {
    const ID: MsgID = MsgID(4007);
}