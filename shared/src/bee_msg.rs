//! BeeGFS network message definitions

use crate::bee_serde::*;
use crate::types::*;
use anyhow::{Context, Result, anyhow};
use bee_serde_derive::BeeSerde;
use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

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
    /// How long to wait to receive this message as a response
    const RESPONSE_TIME_LIMIT: Duration = Duration::from_secs(5);
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Header {
    /// Total length of the serialized message, including the header in the format it was
    /// (de-)serialized with
    msg_len: u32,
    /// Sometimes used for additional message specific payload and/or serialization info
    pub msg_feature_flags: u16,
    /// Sometimes used for additional message specific payload and/or serialization info
    pub msg_compat_feature_flags: u8,
    /// Sometimes used for additional message specific payload and/or serialization info
    pub msg_flags: u8,
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
    pub const LEN: usize = 28;
    /// The serialized length of the legacy header
    pub const LEGACY_LEN: usize = 40;
    /// Fixed value for identifying legacy BeeMsges. In theory, this has some kind of version
    /// modifier (thus the + 0), but it is unused
    #[allow(clippy::identity_op)]
    pub const LEGACY_MSG_PREFIX: u64 = (0x42474653 << 32) + 0;

    /// The total length of the serialized message
    pub fn msg_len(&self) -> usize {
        self.msg_len as usize
    }

    /// The messages id
    pub fn msg_id(&self) -> MsgId {
        self.msg_id
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
    let mut ser = Serializer::new(buf);
    ser.u16(header.msg_id)?;
    ser.u16(header.msg_feature_flags)?;
    ser.u8(header.msg_compat_feature_flags)?;
    ser.u8(header.msg_flags)?;
    ser.u16(header.msg_target_id)?;
    ser.u32(header.msg_user_id)?;
    ser.u64(header.msg_seq)?;
    ser.u64(header.msg_seq_done)?;

    Ok(ser.bytes_written())
}

/// Serializes a legacy BeeMsg header into the provided buffer.
///
/// # Return value
/// Returns the number of bytes written.
pub fn serialize_legacy_header(header: &Header, buf: &mut [u8]) -> Result<usize> {
    let mut ser = Serializer::new(buf);
    ser.u32(header.msg_len)?;
    ser.u16(header.msg_feature_flags)?;
    ser.u8(header.msg_compat_feature_flags)?;
    ser.u8(header.msg_flags)?;
    ser.u64(Header::LEGACY_MSG_PREFIX)?;
    ser.u16(header.msg_id)?;
    ser.u16(header.msg_target_id)?;
    ser.u32(header.msg_user_id)?;
    ser.u64(header.msg_seq)?;
    ser.u64(header.msg_seq_done)?;

    Ok(ser.bytes_written())
}

fn serialize_with<M: Msg + Serializable>(
    msg: &M,
    buf: &mut [u8],
    header_len: usize,
    serialize_header: fn(&Header, &mut [u8]) -> Result<usize>,
) -> Result<usize> {
    let (header_buf, body_buf) = buf
        .split_at_mut_checked(header_len)
        .context("BeeMsg buffer is too small")?;
    let (written, mut header) = serialize_body(msg, body_buf)?;

    header.msg_len = (written + header_len)
        .try_into()
        .context("BeeMsg is too long")?;
    header.msg_id = M::ID;

    serialize_header(&header, header_buf).context("BeeMsg header serialization failed")?;

    Ok(header.msg_len())
}

/// Serializes a complete BeeMsg (header + body) into the provided buffer.
///
/// # Return value
/// Returns the number of bytes written.
pub fn serialize<M: Msg + Serializable>(msg: &M, buf: &mut [u8]) -> Result<usize> {
    serialize_with(msg, buf, Header::LEN, serialize_header)
}

/// Serializes a complete BeeMsg (legacy header + body) into the provided buffer.
///
/// # Return value
/// Returns the number of bytes written.
pub fn serialize_legacy<M: Msg + Serializable>(msg: &M, buf: &mut [u8]) -> Result<usize> {
    serialize_with(msg, buf, Header::LEGACY_LEN, serialize_legacy_header)
}

/// Deserializes a BeeMsg header from the provided buffer.
///
/// # Return value
/// Returns the deserialized header.
pub fn deserialize_header(buf: &[u8]) -> Result<Header> {
    const CTX: &str = "BeeMsg header deserialization failed";

    if buf.len() < Header::LEN {
        return Err(anyhow!(
            "BeeMsg must be at least {} bytes long, got {}",
            Header::LEN,
            buf.len()
        ))
        .context(CTX);
    }

    let mut des = Deserializer::new(&buf[..Header::LEN]);
    let header = (|| {
        Ok::<_, anyhow::Error>(Header {
            msg_id: des.u16()?,
            msg_feature_flags: des.u16()?,
            msg_compat_feature_flags: des.u8()?,
            msg_flags: des.u8()?,
            msg_target_id: des.u16()?,
            msg_user_id: des.u32()?,
            msg_seq: des.u64()?,
            msg_seq_done: des.u64()?,
            msg_len: buf.len().try_into().context("BeeMsg is too long")?,
        })
    })()
    .context(CTX)?;
    des.finish().context(CTX)?;

    Ok(header)
}

/// Deserializes a legacy BeeMsg header from the provided buffer.
///
/// The function checks on wether the reported message length fits into the buffer. Thus, the whole
/// buffer must be passed.
///
/// # Return value
/// Returns the deserialized header.
pub fn deserialize_legacy_header(buf: &[u8]) -> Result<Header> {
    const CTX: &str = "Legacy BeeMsg header deserialization failed";

    let header_buf = buf
        .get(..Header::LEGACY_LEN)
        .ok_or_else(|| {
            anyhow!(
                "Header buffer must be at least {} bytes big, got {}",
                Header::LEGACY_LEN,
                buf.len()
            )
        })
        .context(CTX)?;

    let mut des = Deserializer::new(header_buf);
    let (header, msg_prefix) = (|| {
        let msg_len = des.u32()?;
        let msg_feature_flags = des.u16()?;
        let msg_compat_feature_flags = des.u8()?;
        let msg_flags = des.u8()?;
        let msg_prefix = des.u64()?;
        Ok::<_, anyhow::Error>((
            Header {
                msg_len,
                msg_feature_flags,
                msg_compat_feature_flags,
                msg_flags,
                msg_id: des.u16()?,
                msg_target_id: des.u16()?,
                msg_user_id: des.u32()?,
                msg_seq: des.u64()?,
                msg_seq_done: des.u64()?,
            },
            msg_prefix,
        ))
    })()
    .context(CTX)?;
    des.finish().context(CTX)?;

    if msg_prefix != Header::LEGACY_MSG_PREFIX {
        return Err(anyhow!(
            "Invalid BeeMsg prefix: Must be {}, got {}",
            Header::LEGACY_MSG_PREFIX,
            msg_prefix
        ))
        .context(CTX);
    }

    if header.msg_len() < Header::LEGACY_LEN {
        return Err(anyhow!(
            "Received BeeMsg is shorter than its header: Reported length {}",
            header.msg_len
        ))
        .context(CTX);
    }

    if header.msg_len() > buf.len() {
        return Err(anyhow!(
            "Received BeeMsg doesn't fit into the provided buffer: Reported length {}, \
            buffer size is {}",
            header.msg_len,
            buf.len()
        ))
        .context(CTX);
    }

    Ok(header)
}

/// Deserializes a BeeMsg body from the provided buffer.
///
/// # Return value
/// Returns the deserialized message.
pub fn deserialize_body<M: Msg + Deserializable>(header: &Header, buf: &[u8]) -> Result<M> {
    const CTX: &str = "BeeMsg body deserialization failed";

    let mut des = Deserializer::with_header(buf, header);
    let des_msg = M::deserialize(&mut des).context(CTX)?;
    des.finish().context(CTX)?;

    Ok(des_msg)
}

/// Deserializes a complete BeeMsg (legacy header + body) from the provided buffer.
///
/// # Return value
/// Returns the deserialized message.
pub fn deserialize_legacy<M: Msg + Deserializable>(buf: &[u8]) -> Result<M> {
    let header = deserialize_legacy_header(buf)?;
    let msg = deserialize_body(&header, &buf[Header::LEGACY_LEN..header.msg_len()])?;
    Ok(msg)
}
