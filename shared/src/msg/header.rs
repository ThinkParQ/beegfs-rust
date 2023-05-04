use super::*;
use anyhow::bail;

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct Header {
    pub msg_len: u32,
    pub msg_feature_flags: u16,
    pub msg_compat_feature_flags: u8,
    pub msg_flags: u8,
    pub msg_prefix: u64,
    pub msg_id: MsgID, // Message type as defined in NetMessageTypes.h
    pub msg_target_id: TargetID,
    pub msg_user_id: u32,
    pub msg_seq: u64,
    pub msg_seq_done: u64,
}

impl Header {
    pub const LEN: usize = 40;
    pub const DATA_VERSION: u64 = 0;
    pub const MSG_PREFIX: u64 = (0x42474653 << 32) + Self::DATA_VERSION;

    pub fn new(body_len: usize, msg_id: MsgID, msg_feature_flags: u16) -> Self {
        Self {
            msg_len: (body_len + Self::LEN) as u32,
            msg_feature_flags,
            msg_compat_feature_flags: 0,
            msg_flags: 0,
            msg_prefix: Self::MSG_PREFIX,
            msg_id,
            msg_target_id: TargetID::ZERO,
            msg_user_id: u32::MAX,
            msg_seq: 0,
            msg_seq_done: 0,
        }
    }

    pub fn from_buf(buf: &[u8]) -> Result<Self> {
        if buf.len() != Self::LEN {
            bail!("Header buffer has an unexpected size of {}", buf.len());
        }

        let mut des = Deserializer::new(buf, 0);
        Header::deserialize(&mut des)
    }

    pub fn msg_len(&self) -> usize {
        self.msg_len as usize
    }
}
