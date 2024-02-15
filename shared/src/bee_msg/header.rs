/// Defines the BeeGFS message header
use super::*;
use crate::bee_serde::Deserializer;
use anyhow::bail;

/// The BeeGFS message header
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct Header {
    /// Total length of the message, including the header.
    ///
    /// This determines the amount of bytes read and written from and to sockets.
    pub msg_len: u32,
    /// Sometimes used for additional message specific payload and/or serialization info
    pub msg_feature_flags: u16,
    pub msg_compat_feature_flags: u8,
    pub msg_flags: u8,
    /// Fixed value
    pub msg_prefix: u64,
    /// Uniquely identifies the message type as defined in the C++ codebase in NetMessageTypes.h
    pub msg_id: MsgID,
    pub msg_target_id: TargetID,
    pub msg_user_id: u32,
    pub msg_seq: u64,
    pub msg_seq_done: u64,
}

impl Header {
    pub const LEN: usize = 40;
    pub const DATA_VERSION: u64 = 0;
    pub const MSG_PREFIX: u64 = (0x42474653 << 32) + Self::DATA_VERSION;

    /// Creates a new BeeGFS message header
    ///
    /// `msg_feature_flags` has to be set depending on the message.
    pub fn new(body_len: usize, msg_id: MsgID, msg_feature_flags: u16) -> Self {
        Self {
            msg_len: (body_len + Self::LEN) as u32,
            msg_feature_flags,
            msg_compat_feature_flags: 0,
            msg_flags: 0,
            msg_prefix: Self::MSG_PREFIX,
            msg_id,
            msg_target_id: 0,
            msg_user_id: u32::MAX,
            msg_seq: 0,
            msg_seq_done: 0,
        }
    }

    /// Deserializes the given buffer into a header
    pub fn from_buf(buf: &[u8]) -> Result<Self> {
        if buf.len() != Self::LEN {
            bail!("Header buffer has an unexpected size of {}", buf.len());
        }

        let mut des = Deserializer::new(buf, 0);
        let des_header = Header::deserialize(&mut des)?;
        des.finish()?;
        Ok(des_header)
    }

    /// The expected total message length this header belongs to
    pub fn msg_len(&self) -> usize {
        self.msg_len as usize
    }
}
