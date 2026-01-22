//! BeeGFS network message definitions

use crate::bee_serde::*;
use crate::crypto::AesEncryptionInfo;
use crate::types::*;
use anyhow::{Context, Result, anyhow};
use bee_serde_derive::BeeSerde;
use std::any::Any;
use std::collections::{HashMap, HashSet};

pub mod buddy_group;
pub mod misc;
pub mod node;
pub mod quota;
pub mod storage_pool;
pub mod target;

/// The BeeGFS message ID as defined in `NetMsgTypes.h`
pub type MsgId = u16;

pub trait BaseMsg: Any + std::fmt::Debug + Send + Sync + 'static {}

/// A BeeGFS message
///
/// A struct that implements `Msg` represents a BeeGFS message that is compatible with other C/C++
/// based BeeGFS components.
pub trait Msg: BaseMsg + Default + Clone {
    /// Message type as defined in NetMessageTypes.h
    const ID: MsgId;
}

impl<M> BaseMsg for M where M: Msg {}

/// Matches the `FhgfsOpsErr` value from the BeeGFS C/C++ codebase.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct OpsErr(i32);

impl OpsErr {
    pub const SUCCESS: Self = Self(0);
    pub const INTERNAL: Self = Self(1);
    pub const UNKNOWN_NODE: Self = Self(5);
    pub const EXISTS: Self = Self(7);
    pub const NOTEMPTY: Self = Self(13);
    pub const UNKNOWN_TARGET: Self = Self(15);
    pub const INVAL: Self = Self(20);
    pub const AGAIN: Self = Self(22);
    pub const UNKNOWN_POOL: Self = Self(30);
}

/// The BeeMsg header
#[derive(Clone, Debug, PartialEq, Eq, BeeSerde)]
pub struct Header {
    /// Total length of the serialized message, including the header itself
    msg_len: u32,
    /// Signature field
    msg_encryption_info: AesEncryptionInfo,
    /// Sometimes used for additional message specific payload and/or serialization info
    pub msg_feature_flags: u16,
    /// Sometimes used for additional message specific payload and/or serialization info
    pub msg_compat_feature_flags: u8,
    /// Sometimes used for additional message specific payload and/or serialization info
    pub msg_flags: u8,
    /// Fixed value to identify a BeeMsg header (see MSG_PREFIX below)
    msg_prefix: u64,
    /// Uniquely identifies the message type as defined in the C++ codebase in NetMessageTypes.h
    msg_id: MsgId,
    /// Sometimes used for additional message specific payload and/or serialization info
    pub msg_target_id: TargetId,
    /// Sometimes used for additional message specific payload and/or serialization info
    pub msg_user_id: u32,
    /// Mirroring related information
    pub msg_seq: u64,
    /// Mirroring related information
    pub msg_seq_done: u64,
}

impl Header {
    /// The serialized length of the header
    pub const LEN: usize = 68;
    /// The length of the unencrypted part at the start of the header
    pub const ENCRYPTION_INFO_LEN: usize = 32;

    /// Fixed value for identifying BeeMsges. In theory, this has some kind of version modifier
    /// (thus the + 0), but it is unused
    #[allow(clippy::identity_op)]
    pub const MSG_PREFIX: u64 = (0x42474653 << 32) + 0;

    /// The total length of the serialized message
    pub fn msg_len(&self) -> usize {
        self.msg_len as usize
    }

    /// The messages id
    pub fn msg_id(&self) -> MsgId {
        self.msg_id
    }
}

impl Default for Header {
    fn default() -> Self {
        Self {
            msg_len: 0,
            msg_encryption_info: AesEncryptionInfo::default(),
            msg_feature_flags: 0,
            msg_compat_feature_flags: 0,
            msg_flags: 0,
            msg_prefix: Self::MSG_PREFIX,
            msg_id: 0,
            msg_target_id: 0,
            msg_user_id: 0,
            msg_seq: 0,
            msg_seq_done: 0,
        }
    }
}

/// Serializes a BeeMsg body into the provided buffer.
///
/// The data is written from the beginning of the slice, it's up to the caller to pass the correct
/// sub slice if space for the header should be reserved.
///
/// # Return value
/// Returns the number of bytes written and the header modified by serialization function.
pub fn serialize_body<M: Msg + Serializable>(msg: &M, buf: &mut [u8]) -> Result<(usize, Header)> {
    let mut ser = Serializer::new(buf);
    msg.serialize(&mut ser)
        .context("BeeMsg body serialization failed")?;

    Ok((ser.bytes_written(), ser.finish()))
}

/// Serializes a BeeMsg header into the provided buffer.
///
/// # Return value
/// Returns the number of bytes written.
pub fn serialize_header(header: &Header, buf: &mut [u8]) -> Result<usize> {
    let mut ser_header = Serializer::new(buf);
    header
        .serialize(&mut ser_header)
        .context("BeeMsg header serialization failed")?;

    Ok(ser_header.bytes_written())
}

/// Serializes a complete BeeMsg (header + body) into the provided buffer.
///
/// # Return value
/// Returns the number of bytes written.
pub fn serialize<M: Msg + Serializable>(msg: &M, buf: &mut [u8]) -> Result<usize> {
    let (written, mut header) = serialize_body(msg, &mut buf[Header::LEN..])?;

    header.msg_len = (written + Header::LEN) as u32;
    header.msg_id = M::ID;

    let _ = serialize_header(&header, &mut buf[0..Header::LEN])?;

    Ok(header.msg_len())
}

pub fn deserialize_encryption_header(buf: &[u8]) -> Result<(usize, AesEncryptionInfo)> {
    const CTX: &str = "BeeMsg encryption header deserialization failed";

    let buf = buf
        .get(..Header::ENCRYPTION_INFO_LEN)
        .ok_or_else(|| {
            anyhow!(
                "Cipher header buffer must be at least {} bytes big, got {}",
                Header::ENCRYPTION_INFO_LEN,
                buf.len()
            )
        })
        .context(CTX)?;

    let mut des = Deserializer::new(buf);
    let msg_length = des.u32().context(CTX)?;
    let info = AesEncryptionInfo::deserialize(&mut des).context(CTX)?;
    des.finish().context(CTX)?;

    Ok((msg_length.try_into().context(CTX)?, info))
}

/// Deserializes a BeeMsg header from the provided buffer.
///
/// # Return value
/// Returns the deserialized header.
pub fn deserialize_header(buf: &[u8]) -> Result<Header> {
    const CTX: &str = "BeeMsg header deserialization failed";

    let header_buf = buf
        .get(..Header::LEN)
        .ok_or_else(|| {
            anyhow!(
                "Header buffer must be at least {} bytes big, got {}",
                Header::LEN,
                buf.len()
            )
        })
        .context(CTX)?;

    let mut des = Deserializer::new(header_buf);
    let header = Header::deserialize(&mut des).context(CTX)?;
    des.finish().context(CTX)?;

    if header.msg_prefix != Header::MSG_PREFIX {
        return Err(anyhow!(
            "Invalid BeeMsg prefix: Must be {}, got {}",
            Header::MSG_PREFIX,
            header.msg_prefix
        ))
        .context(CTX);
    }

    Ok(header)
}

/// Deserializes a BeeMsg body from the provided buffer.
///
/// The data is read from the beginning of the slice, it's up to the caller to pass the correct
/// sub slice if space for the header should be excluded from the source.
///
/// # Return value
/// Returns the deserialized message.
pub fn deserialize_body<M: Msg + Deserializable>(header: &Header, buf: &[u8]) -> Result<M> {
    const CTX: &str = "BeeMsg body deserialization failed";

    let mut des = Deserializer::with_header(&buf[0..(header.msg_len() - Header::LEN)], header);
    let des_msg = M::deserialize(&mut des).context(CTX)?;
    des.finish().context(CTX)?;

    Ok(des_msg)
}

/// Deserializes a complete BeeMsg (header + body) from the provided buffer.
///
/// # Return value
/// Returns the deserialized message.
pub fn deserialize<M: Msg + Deserializable>(buf: &[u8]) -> Result<M> {
    let header = deserialize_header(&buf[0..Header::LEN])?;
    let msg = deserialize_body(&header, &buf[Header::LEN..])?;
    Ok(msg)
}
