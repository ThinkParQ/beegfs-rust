//! BeeGFS network message definitions

use crate::bee_serde::*;
use crate::protocol::{FRAME_HEADER_LEN, FrameHeader, FrameType, Protocol};
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
#[derive(Clone, Debug, PartialEq, Eq, BeeSerde)]
pub struct Header {
    /// Total length of the serialized message, including the header itself
    msg_len: u32,
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
    pub const LEN: usize = 40;
    /// Fixed value for identifying BeeMsges. In theory, this has some kind of version modifier
    /// (thus the + 0), but it is unused
    #[allow(clippy::identity_op)]
    pub const MSG_PREFIX: u64 = (0x42474653 << 32) + 0;
    /// The serialized length of the header part carried by the new protocol, which drops
    /// `msg_len` and `msg_prefix` in favor of the frame header.
    pub const LEN_V2: usize = 28;

    /// The total length of the serialized message
    pub fn msg_len(&self) -> usize {
        self.msg_len as usize
    }

    /// The messages id
    pub fn msg_id(&self) -> MsgId {
        self.msg_id
    }

    /// Serializes the header as laid out by the new protocol.
    ///
    /// `msg_len` and `msg_prefix` are omitted - the frame header carries both.
    fn serialize_v2(&self, ser: &mut Serializer<'_>) -> Result<()> {
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

    /// Counterpart of [`Header::serialize_v2`]. Leaves `msg_len` at 0 for the caller to fill in
    /// from the frame length.
    // Fields are read in wire order - a struct literal evaluates its fields top to bottom.
    fn deserialize_v2(des: &mut Deserializer<'_>) -> Result<Self> {
        Ok(Self {
            msg_len: 0,
            msg_feature_flags: des.u16()?,
            msg_compat_feature_flags: des.u8()?,
            msg_flags: des.u8()?,
            msg_id: des.u16()?,
            msg_target_id: des.u16()?,
            msg_user_id: des.u32()?,
            msg_seq: des.u64()?,
            msg_seq_done: des.u64()?,
            msg_prefix: Self::MSG_PREFIX,
        })
    }
}

/// The body must start at the same offset in both protocols, so that `msg_len` keeps meaning
/// "total framed length" and the body length stays `msg_len - Header::LEN` either way.
const _: () = assert!(FRAME_HEADER_LEN + Header::LEN_V2 == Header::LEN);

impl Default for Header {
    fn default() -> Self {
        Self {
            msg_len: 0,
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

/// Serializes a complete BeeMsg (frame header if any + header + body) into the provided buffer.
///
/// # Return value
/// Returns the number of bytes written.
pub fn serialize<M: Msg + Serializable>(
    msg: &M,
    protocol: Protocol,
    buf: &mut [u8],
) -> Result<usize> {
    let (written, mut header) = serialize_body(msg, &mut buf[Header::LEN..])?;

    header.msg_len = (written + Header::LEN) as u32;
    header.msg_id = M::ID;

    if protocol.is_legacy() {
        serialize_header(&header, &mut buf[0..Header::LEN])?;
    } else {
        // Written for an unprotected payload. The send path patches the length when the payload
        // goes out as Noise records.
        FrameHeader {
            ftype: FrameType::Message,
            protected: false,
            payload_len: Header::LEN_V2 + written,
        }
        .encode(&mut buf[0..FRAME_HEADER_LEN])?;

        let mut ser = Serializer::new(&mut buf[FRAME_HEADER_LEN..Header::LEN]);
        header.serialize_v2(&mut ser)?;
    }

    Ok(header.msg_len())
}

/// Deserializes a BeeMsg header from the provided buffer.
///
/// The function checks on wether the reported message length fits into the buffer. Thus, the whole
/// buffer must be passed.
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

    if header.msg_len as usize > buf.len() {
        return Err(anyhow!(
            "Received BeeMsg doesn't fit into the provided buffer: Reported length {}, \
            buffer size is {}",
            header.msg_len,
            buf.len()
        ))
        .context(CTX);
    }

    // Without this the read paths would slice buf[Header::LEN..msg_len] with start > end.
    ensure!(
        header.msg_len() >= Header::LEN,
        "{CTX}: msg_len {} is smaller than the header length {}",
        header.msg_len(),
        Header::LEN
    );

    Ok(header)
}

/// Deserializes the new protocol's BeeMsg header from the provided buffer.
///
/// `buf` must start at the BeeMsg header, so after the frame header. `payload_len` is the frame
/// payload length, which is where `msg_len` comes from.
///
/// # Return value
/// Returns the deserialized header.
pub fn deserialize_header_v2(buf: &[u8], payload_len: usize) -> Result<Header> {
    const CTX: &str = "BeeMsg header deserialization failed";

    ensure!(
        payload_len >= Header::LEN_V2,
        "{CTX}: a frame payload of {payload_len} bytes cannot hold a {} byte header",
        Header::LEN_V2
    );
    ensure!(
        payload_len <= buf.len(),
        "{CTX}: reported payload length {payload_len} exceeds the buffer size {}",
        buf.len()
    );

    let header_buf = buf
        .get(..Header::LEN_V2)
        .ok_or_else(|| {
            anyhow!(
                "Header buffer must be at least {} bytes big, got {}",
                Header::LEN_V2,
                buf.len()
            )
        })
        .context(CTX)?;

    let mut des = Deserializer::new(header_buf);
    let mut header = Header::deserialize_v2(&mut des).context(CTX)?;
    des.finish().context(CTX)?;

    header.msg_len = (FRAME_HEADER_LEN + payload_len) as u32;

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
        .checked_sub(Header::LEN)
        .ok_or_else(|| {
            anyhow!(
                "msg_len {} is smaller than the header length {}",
                header.msg_len(),
                Header::LEN
            )
        })
        .context(CTX)?;

    let body_buf = buf
        .get(..body_len)
        .ok_or_else(|| {
            anyhow!(
                "Body buffer must be at least {body_len} bytes big, got {}",
                buf.len()
            )
        })
        .context(CTX)?;

    let mut des = Deserializer::with_header(body_buf, header);
    let des_msg = M::deserialize(&mut des).context(CTX)?;
    des.finish().context(CTX)?;

    Ok(des_msg)
}

/// Deserializes a complete BeeMsg (header + body) from the provided buffer.
///
/// # Return value
/// Returns the deserialized message.
pub fn deserialize<M: Msg + Deserializable>(buf: &[u8]) -> Result<M> {
    let header = deserialize_header(buf)?;
    let msg = deserialize_body(&header, &buf[Header::LEN..])?;
    Ok(msg)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::bee_msg::misc::AuthenticateChannel;
    use std::str::FromStr;

    /// A message with a small, fully determined body.
    fn test_msg() -> AuthenticateChannel {
        AuthenticateChannel {
            // 0x0102030405060708, so every body byte is distinct.
            auth_secret: AuthSecret::from_str("72623859790382856").unwrap(),
        }
    }

    const MSG_ID: [u8; 2] = [0xa7, 0x0f]; // 4007
    const BODY: [u8; 8] = [8, 7, 6, 5, 4, 3, 2, 1];

    /// Pins the legacy wire format. Other BeeGFS implementations parse exactly these bytes, so a
    /// change here is a compatibility break and must be deliberate.
    #[test]
    fn legacy_golden_bytes() {
        #[rustfmt::skip]
        let expected: [u8; 48] = [
            48, 0, 0, 0,                    // msg_len = 40 + 8
            0, 0,                           // msg_feature_flags
            0,                              // msg_compat_feature_flags
            0,                              // msg_flags
            0, 0, 0, 0, 0x53, 0x46, 0x47, 0x42, // msg_prefix
            MSG_ID[0], MSG_ID[1],           // msg_id
            0, 0,                           // msg_target_id
            0, 0, 0, 0,                     // msg_user_id
            0, 0, 0, 0, 0, 0, 0, 0,         // msg_seq
            0, 0, 0, 0, 0, 0, 0, 0,         // msg_seq_done
            BODY[0], BODY[1], BODY[2], BODY[3], BODY[4], BODY[5], BODY[6], BODY[7],
        ];

        let mut buf = [0u8; 64];
        let len = serialize(&test_msg(), Protocol::Legacy, &mut buf).unwrap();

        assert_eq!(len, expected.len());
        assert_eq!(&buf[..len], &expected);
    }

    /// Pins the new wire format against the layout the C++ tree and kernel client must match.
    #[test]
    fn new_protocol_golden_bytes() {
        #[rustfmt::skip]
        let expected: [u8; 48] = [
            b'B', b'G', b'F', b'S',         // frame magic
            36, 0, 0, 0,                    // payload_len = 28 + 8
            4,                              // frame_type = Message
            0,                              // frame_flags
            0, 0,                           // reserved
            0, 0,                           // msg_feature_flags
            0,                              // msg_compat_feature_flags
            0,                              // msg_flags
            MSG_ID[0], MSG_ID[1],           // msg_id
            0, 0,                           // msg_target_id
            0, 0, 0, 0,                     // msg_user_id
            0, 0, 0, 0, 0, 0, 0, 0,         // msg_seq
            0, 0, 0, 0, 0, 0, 0, 0,         // msg_seq_done
            BODY[0], BODY[1], BODY[2], BODY[3], BODY[4], BODY[5], BODY[6], BODY[7],
        ];

        for protocol in [
            Protocol::Plain,
            Protocol::Authenticated,
            Protocol::Encrypted,
        ] {
            let mut buf = [0u8; 64];
            let len = serialize(&test_msg(), protocol, &mut buf).unwrap();

            assert_eq!(len, expected.len());
            assert_eq!(&buf[..len], &expected, "protocol {protocol:?}");
        }
    }

    #[test]
    fn round_trip() {
        let msg = test_msg();

        let mut buf = [0u8; 64];
        let len = serialize(&msg, Protocol::Legacy, &mut buf).unwrap();
        let header = deserialize_header(&buf).unwrap();
        assert_eq!(header.msg_len(), len);
        assert_eq!(header.msg_id(), AuthenticateChannel::ID);
        assert_eq!(
            deserialize_body::<AuthenticateChannel>(&header, &buf[Header::LEN..]).unwrap(),
            msg
        );

        let mut buf = [0u8; 64];
        let len = serialize(&msg, Protocol::Plain, &mut buf).unwrap();
        let frame = FrameHeader::decode(&buf).unwrap();
        assert_eq!(frame.ftype, FrameType::Message);
        assert!(!frame.protected);
        let header = deserialize_header_v2(&buf[FRAME_HEADER_LEN..], frame.payload_len).unwrap();
        assert_eq!(header.msg_len(), len);
        assert_eq!(header.msg_id(), AuthenticateChannel::ID);
        assert_eq!(
            deserialize_body::<AuthenticateChannel>(&header, &buf[Header::LEN..]).unwrap(),
            msg
        );
    }

    /// Both protocols keep the body at the same offset, which is what lets `msg_len` and
    /// `deserialize_body` stay protocol agnostic.
    #[test]
    fn body_offset_is_protocol_independent() {
        let mut legacy = [0u8; 64];
        let mut new = [0u8; 64];

        let legacy_len = serialize(&test_msg(), Protocol::Legacy, &mut legacy).unwrap();
        let new_len = serialize(&test_msg(), Protocol::Plain, &mut new).unwrap();

        assert_eq!(legacy_len, new_len);
        assert_eq!(&legacy[Header::LEN..legacy_len], &new[Header::LEN..new_len]);
    }

    /// A peer controls these lengths, so they must produce errors rather than panics.
    #[test]
    fn rejects_out_of_range_lengths() {
        let mut buf = [0u8; 64];
        serialize(&test_msg(), Protocol::Legacy, &mut buf).unwrap();

        // Too small to even hold the header.
        for msg_len in [0u32, 1, (Header::LEN - 1) as u32] {
            let mut short = buf;
            short[0..4].copy_from_slice(&msg_len.to_le_bytes());
            deserialize_header(&short).unwrap_err();
        }

        // Bigger than the buffer it is supposed to arrive in.
        let mut long = buf;
        long[0..4].copy_from_slice(&((buf.len() + 1) as u32).to_le_bytes());
        deserialize_header(&long).unwrap_err();

        // Passes the header check, but the body does not fit the buffer handed over.
        let mut body = buf;
        body[0..4].copy_from_slice(&(buf.len() as u32).to_le_bytes());
        let header = deserialize_header(&body).unwrap();
        deserialize_body::<AuthenticateChannel>(&header, &body[Header::LEN..Header::LEN + 8])
            .unwrap_err();

        // New protocol: a payload too small to hold the header, or bigger than the buffer.
        let mut buf = [0u8; 64];
        serialize(&test_msg(), Protocol::Plain, &mut buf).unwrap();
        deserialize_header_v2(&buf[FRAME_HEADER_LEN..], Header::LEN_V2 - 1).unwrap_err();
        deserialize_header_v2(&buf[FRAME_HEADER_LEN..], buf.len()).unwrap_err();
    }
}
