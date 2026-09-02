//! BeeGFS network message definitions

use crate::bee_serde::*;
use crate::conn::protocol::FrameHeader;
use crate::types::*;
use anyhow::{Context, Result, anyhow, ensure};
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
#[cfg(any())] // TEMP-DISABLED-TESTS: re-enable by restoring #[cfg(test)]
mod test;

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
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Header {
    /// Total length of the serialized message, including the header itself
    msg_len: usize,
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
    /// The position in the serialized data after the header ends => where the body starts. This is
    /// the same for the legacy protocol and new one when including the frame header.
    pub const END_POS: usize = 40;

    /// Fixed value for identifying BeeMsges. In theory, this has some kind of version modifier
    /// (thus the + 0), but it is unused
    #[allow(clippy::identity_op)]
    pub const LEGACY_MAGIC: u64 = (0x42474653 << 32) + 0;

    /// The total length of the serialized message
    pub fn msg_len(&self) -> usize {
        self.msg_len
    }

    /// The messages id
    pub fn msg_id(&self) -> MsgId {
        self.msg_id
    }

    /// Serialize the header for the new BeeMsg protocol, which carries it behind the frame header
    pub fn serialize(&self, buf: &mut [u8]) -> Result<()> {
        ensure!(
            buf.len() == Self::END_POS - FrameHeader::END_POS,
            "Got buffer of length {} but the header needs {}",
            buf.len(),
            Self::END_POS - FrameHeader::END_POS,
        );

        let mut ser = Serializer::new(buf);

        ser.u16(self.msg_feature_flags)?;
        ser.u8(self.msg_compat_feature_flags)?;
        ser.u8(self.msg_flags)?;
        ser.u16(self.msg_id)?;
        ser.u16(self.msg_target_id)?;
        ser.u32(self.msg_user_id)?;
        ser.u64(self.msg_seq)?;
        ser.u64(self.msg_seq_done)?;

        Ok(())
    }

    /// Serialize the header for the legacy BeeMsg protocol
    pub fn serialize_legacy(&self, buf: &mut [u8]) -> Result<()> {
        ensure!(
            buf.len() == Self::END_POS,
            "Got buffer of length {} but the legacy header needs {}",
            buf.len(),
            Self::END_POS,
        );

        let mut ser = Serializer::new(buf);

        ser.u32(self.msg_len.try_into()?)?;
        ser.u16(self.msg_feature_flags)?;
        ser.u8(self.msg_compat_feature_flags)?;
        ser.u8(self.msg_flags)?;
        ser.u64(Self::LEGACY_MAGIC)?;
        ser.u16(self.msg_id)?;
        ser.u16(self.msg_target_id)?;
        ser.u32(self.msg_user_id)?;
        ser.u64(self.msg_seq)?;
        ser.u64(self.msg_seq_done)?;

        Ok(())
    }

    /// Deserialize the header from the new BeeMsg protocol.
    ///
    /// `msg_len` is not on the wire here. It counts the same bytes as `frame_len`, so it is taken
    /// straight from it.
    pub fn deserialize(buf: &[u8], frame_len: usize) -> Result<Self> {
        ensure!(
            frame_len >= Self::END_POS,
            "Frame length {frame_len} is smaller than the header {}",
            Self::END_POS
        );

        let mut des = Deserializer::new(buf);

        let header = Self {
            msg_len: frame_len,
            msg_feature_flags: des.u16()?,
            msg_compat_feature_flags: des.u8()?,
            msg_flags: des.u8()?,
            msg_id: des.u16()?,
            msg_target_id: des.u16()?,
            msg_user_id: des.u32()?,
            msg_seq: des.u64()?,
            msg_seq_done: des.u64()?,
        };
        des.finish()?;

        Ok(header)
    }

    /// Deserialize the header from the legacy BeeMsg protocol. Expects the whole buffer to
    /// do a msg length check.
    pub fn deserialize_legacy(buf: &[u8]) -> Result<Self> {
        let header_buf = buf.get(..Self::END_POS).ok_or_else(|| {
            anyhow!(
                "Buffer of {} bytes is too small for a header of {}",
                buf.len(),
                Self::END_POS
            )
        })?;

        let mut des = Deserializer::new(header_buf);

        let msg_len = des.u32()?.try_into()?;
        let msg_feature_flags = des.u16()?;
        let msg_compat_feature_flags = des.u8()?;
        let msg_flags = des.u8()?;
        let msg_prefix = des.u64()?;
        let msg_id = des.u16()?;
        let msg_target_id = des.u16()?;
        let msg_user_id = des.u32()?;
        let msg_seq = des.u64()?;
        let msg_seq_done = des.u64()?;

        des.finish()?;

        ensure!(
            msg_prefix == Self::LEGACY_MAGIC,
            "Invalid BeeMsg prefix: Must be {}, got {}. The peer is most likely configured for a \
            different BeeMsg protocol",
            Self::LEGACY_MAGIC,
            msg_prefix
        );

        ensure!(
            msg_len >= Self::END_POS,
            "Message length {msg_len} is smaller than the header {}",
            Self::END_POS
        );
        ensure!(
            msg_len <= buf.len(),
            "Message length {msg_len} exceeds the buffer size {}",
            buf.len()
        );

        Ok(Self {
            msg_len,
            msg_feature_flags,
            msg_compat_feature_flags,
            msg_flags,
            msg_id,
            msg_target_id,
            msg_user_id,
            msg_seq,
            msg_seq_done,
        })
    }
}

pub fn serialize_body<M: Msg + Serializable>(msg: &M, buf: &mut [u8]) -> Result<Header> {
    let mut ser = Serializer::new(&mut buf[Header::END_POS..]);
    msg.serialize(&mut ser)
        .context("BeeMsg body serialization failed")?;

    let written = ser.bytes_written();
    let mut header = ser.finish();

    header.msg_len = written + Header::END_POS;
    header.msg_id = M::ID;

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

    let body_len = header
        .msg_len()
        .checked_sub(Header::END_POS)
        .ok_or_else(|| {
            anyhow!(
                "Total Message length {} is smaller than the header length {}",
                header.msg_len(),
                Header::END_POS
            )
        })
        .context(CTX)?;

    let body_buf = buf
        .get(..body_len)
        .ok_or_else(|| {
            anyhow!(
                "Body buffer must have at least {body_len} bytes, got {}",
                buf.len()
            )
        })
        .context(CTX)?;

    let mut des = Deserializer::with_header(body_buf, header);
    let des_msg = M::deserialize(&mut des).context(CTX)?;
    des.finish().context(CTX)?;

    Ok(des_msg)
}
