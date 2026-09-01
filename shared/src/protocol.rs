//! BeeMsg wire protocol selection and framing.
//!
//! The legacy protocol puts a bare `[Header][body]` sequence on the wire. The new protocol prepends
//! a 12 byte cleartext frame header carrying the protocol specifier, the payload length and a frame
//! type. Moving those to the front is what allows everything behind them to be authenticated and/or
//! encrypted, and lets the key exchange share the connection with ordinary messages.

use anyhow::{Result, bail, ensure};

/// Selects the BeeMsg wire format and the protection applied to it.
///
/// Must match on all nodes of a system - there is no negotiation, a mismatch is a configuration
/// error that surfaces as a failed connection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Protocol {
    /// The pre-8.x wire format with the `AuthenticateChannel` shared secret.
    #[default]
    Legacy,
    /// New framing, no authentication and no encryption.
    Plain,
    /// New framing with a Noise KK handshake at connection setup. Message contents are not
    /// protected afterwards.
    Authenticated,
    /// New framing with a Noise KK handshake and authenticated encryption of every message.
    Encrypted,
}

impl Protocol {
    pub const fn is_legacy(self) -> bool {
        matches!(self, Self::Legacy)
    }

    /// Offset of the BeeMsg header within a message buffer.
    pub const fn msg_offset(self) -> usize {
        match self {
            Self::Legacy => 0,
            _ => FRAME_HEADER_LEN,
        }
    }

    pub const fn needs_handshake(self) -> bool {
        matches!(self, Self::Authenticated | Self::Encrypted)
    }

    pub const fn protects_records(self) -> bool {
        matches!(self, Self::Encrypted)
    }

    /// The protection request sent in the first handshake message.
    pub const fn requested_modes(self) -> u16 {
        if self.protects_records() {
            MODE_ENCRYPT
        } else {
            0
        }
    }
}

/// Identifies a BeeMsg frame. Serialized little endian, so it reads as "BGFS" on the wire.
///
/// Interpreted as a legacy `msg_len` this is far larger than [`crate::conn::TCP_BUF_LEN`], so a
/// legacy peer fed new-protocol bytes fails its length check instead of waiting for data that
/// never comes.
pub const FRAME_MAGIC: u32 = 0x5346_4742;

pub const FRAME_HEADER_LEN: usize = 12;

/// `frame_flags` bit 0: the payload is a sequence of Noise records.
pub const FRAME_FLAG_PROTECTED: u8 = 1 << 0;
/// Reserved for a future authenticate-without-encrypt mode, so it can be added without a framing
/// change.
const FRAME_FLAG_RESERVED: u8 = 1 << 1;
/// Bits a sender must leave unset and a receiver must reject, keeping them usable later.
const FRAME_FLAGS_UNUSED: u8 = !(FRAME_FLAG_PROTECTED | FRAME_FLAG_RESERVED);

/// `modes` bit 0 in the handshake: encrypt the records.
pub const MODE_ENCRYPT: u16 = 1 << 0;

/// snow's `MAXMSGLEN`. Its `constants` module is private, so the value is repeated here.
const NOISE_MAX_MSG_LEN: usize = 65535;

/// snow's `TAGLEN`.
pub const RECORD_TAG_LEN: usize = 16;
/// On-wire size of a full record.
pub const RECORD_LEN: usize = NOISE_MAX_MSG_LEN;
/// Plaintext carried by a full record.
pub const RECORD_PLAINTEXT_LEN: usize = RECORD_LEN - RECORD_TAG_LEN;

/// Largest plaintext a frame can carry: what is left of the stream buffer after the frame header.
pub const MAX_FRAME_PAYLOAD_LEN: usize = crate::conn::TCP_BUF_LEN - FRAME_HEADER_LEN;
/// Largest on-wire payload. Exceeds [`MAX_FRAME_PAYLOAD_LEN`] because record tags are added to it.
pub const MAX_FRAME_LEN: usize = record_frame_len(MAX_FRAME_PAYLOAD_LEN);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameType {
    ClientHello = 1,
    ServerHello = 2,
    Reject = 3,
    Message = 4,
}

impl TryFrom<u8> for FrameType {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> Result<Self> {
        Ok(match value {
            1 => Self::ClientHello,
            2 => Self::ServerHello,
            3 => Self::Reject,
            4 => Self::Message,
            _ => bail!("Unknown BeeMsg frame type {value}"),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameHeader {
    pub ftype: FrameType,
    /// The payload is a sequence of Noise records rather than cleartext.
    pub protected: bool,
    /// Payload bytes following the frame header.
    pub payload_len: usize,
}

impl FrameHeader {
    pub fn encode(&self, buf: &mut [u8]) -> Result<()> {
        const CTX: &str = "BeeMsg frame header serialization failed";

        let buf = buf.get_mut(..FRAME_HEADER_LEN).ok_or_else(|| {
            anyhow::anyhow!("{CTX}: buffer smaller than {FRAME_HEADER_LEN} bytes")
        })?;

        let payload_len: u32 = self
            .payload_len
            .try_into()
            .map_err(|_| anyhow::anyhow!("{CTX}: payload length {} too big", self.payload_len))?;

        buf[0..4].copy_from_slice(&FRAME_MAGIC.to_le_bytes());
        buf[4..8].copy_from_slice(&payload_len.to_le_bytes());
        buf[8] = self.ftype as u8;
        buf[9] = if self.protected {
            FRAME_FLAG_PROTECTED
        } else {
            0
        };
        buf[10..12].copy_from_slice(&0u16.to_le_bytes());

        Ok(())
    }

    pub fn decode(buf: &[u8]) -> Result<Self> {
        const CTX: &str = "BeeMsg frame header deserialization failed";

        let buf = buf.get(..FRAME_HEADER_LEN).ok_or_else(|| {
            anyhow::anyhow!("{CTX}: buffer smaller than {FRAME_HEADER_LEN} bytes")
        })?;

        let magic = u32::from_le_bytes(buf[0..4].try_into()?);
        ensure!(
            magic == FRAME_MAGIC,
            "{CTX}: invalid frame magic {magic:#010x}, expected {FRAME_MAGIC:#010x}. \
The peer is most likely configured for a different BeeMsg protocol"
        );

        let payload_len = u32::from_le_bytes(buf[4..8].try_into()?) as usize;
        ensure!(
            payload_len <= MAX_FRAME_LEN,
            "{CTX}: payload length {payload_len} exceeds the maximum of {MAX_FRAME_LEN}"
        );

        let ftype = FrameType::try_from(buf[8])?;

        let flags = buf[9];
        ensure!(
            flags & FRAME_FLAGS_UNUSED == 0,
            "{CTX}: unused frame flag bits set ({flags:#04x})"
        );
        ensure!(
            flags & FRAME_FLAG_RESERVED == 0,
            "{CTX}: reserved frame flag bit set ({flags:#04x})"
        );

        let reserved = u16::from_le_bytes(buf[10..12].try_into()?);
        ensure!(
            reserved == 0,
            "{CTX}: reserved field is {reserved}, must be 0"
        );

        Ok(Self {
            ftype,
            protected: flags & FRAME_FLAG_PROTECTED != 0,
            payload_len,
        })
    }
}

/// Number of Noise records needed to carry `plaintext_len` bytes.
pub const fn record_count(plaintext_len: usize) -> usize {
    plaintext_len.div_ceil(RECORD_PLAINTEXT_LEN)
}

/// On-wire payload length for `plaintext_len` bytes carried as Noise records.
pub const fn record_frame_len(plaintext_len: usize) -> usize {
    plaintext_len + record_count(plaintext_len) * RECORD_TAG_LEN
}

/// Inverse of [`record_frame_len`].
///
/// Attacker facing: every record but the last must be filled completely, so a peer gets no say
/// over the record boundaries.
pub fn record_plaintext_len(payload_len: usize) -> Result<usize> {
    const CTX: &str = "Invalid BeeMsg record payload length";

    ensure!(
        payload_len > RECORD_TAG_LEN,
        "{CTX}: {payload_len} is too small to hold a record"
    );
    ensure!(
        payload_len <= MAX_FRAME_LEN,
        "{CTX}: {payload_len} exceeds the maximum of {MAX_FRAME_LEN}"
    );

    let records = payload_len.div_ceil(RECORD_LEN);
    let plaintext_len = payload_len - records * RECORD_TAG_LEN;

    ensure!(
        plaintext_len > (records - 1) * RECORD_PLAINTEXT_LEN,
        "{CTX}: {payload_len} does not encode {records} completely filled records"
    );
    ensure!(
        plaintext_len <= MAX_FRAME_PAYLOAD_LEN,
        "{CTX}: plaintext length {plaintext_len} exceeds the maximum of {MAX_FRAME_PAYLOAD_LEN}"
    );

    Ok(plaintext_len)
}

#[cfg(test)]
mod test {
    use super::*;

    /// snow rejects a payload whose ciphertext would exceed `MAXMSGLEN`, so a full record must sit
    /// exactly on that limit. Pins the constants against a snow update.
    #[test]
    fn record_sizes_match_noise_limits() {
        assert_eq!(RECORD_PLAINTEXT_LEN + RECORD_TAG_LEN, NOISE_MAX_MSG_LEN);
        assert_eq!(RECORD_LEN, NOISE_MAX_MSG_LEN);
        assert_eq!(RECORD_PLAINTEXT_LEN, 65519);
    }

    #[test]
    fn frame_header_round_trip() {
        for ftype in [
            FrameType::ClientHello,
            FrameType::ServerHello,
            FrameType::Reject,
            FrameType::Message,
        ] {
            for protected in [false, true] {
                for payload_len in [1, 88, MAX_FRAME_PAYLOAD_LEN, MAX_FRAME_LEN] {
                    let h = FrameHeader {
                        ftype,
                        protected,
                        payload_len,
                    };

                    let mut buf = [0u8; FRAME_HEADER_LEN];
                    h.encode(&mut buf).unwrap();
                    assert_eq!(FrameHeader::decode(&buf).unwrap(), h);
                }
            }
        }
    }

    /// The magic must be the bytes "BGFS" so a peer of the other protocol fails intelligibly.
    #[test]
    fn frame_magic_wire_bytes() {
        let mut buf = [0u8; FRAME_HEADER_LEN];
        FrameHeader {
            ftype: FrameType::Message,
            protected: false,
            payload_len: 0,
        }
        .encode(&mut buf)
        .unwrap();

        assert_eq!(&buf[0..4], b"BGFS");
        assert!(FRAME_MAGIC as usize > crate::conn::TCP_BUF_LEN);
    }

    #[test]
    fn frame_header_rejects_malformed() {
        let good = {
            let mut buf = [0u8; FRAME_HEADER_LEN];
            FrameHeader {
                ftype: FrameType::Message,
                protected: false,
                payload_len: 64,
            }
            .encode(&mut buf)
            .unwrap();
            buf
        };

        FrameHeader::decode(&good).unwrap();
        FrameHeader::decode(&good[..FRAME_HEADER_LEN - 1]).unwrap_err();

        let mut bad_magic = good;
        bad_magic[0] ^= 0xff;
        FrameHeader::decode(&bad_magic).unwrap_err();

        let mut bad_type = good;
        bad_type[8] = 0;
        FrameHeader::decode(&bad_type).unwrap_err();
        bad_type[8] = 5;
        FrameHeader::decode(&bad_type).unwrap_err();

        let mut reserved_flag = good;
        reserved_flag[9] = FRAME_FLAG_RESERVED;
        FrameHeader::decode(&reserved_flag).unwrap_err();

        let mut unused_flag = good;
        unused_flag[9] = 1 << 7;
        FrameHeader::decode(&unused_flag).unwrap_err();

        let mut bad_reserved = good;
        bad_reserved[10] = 1;
        FrameHeader::decode(&bad_reserved).unwrap_err();

        let mut too_long = good;
        too_long[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        FrameHeader::decode(&too_long).unwrap_err();

        let mut just_too_long = good;
        just_too_long[4..8].copy_from_slice(&((MAX_FRAME_LEN + 1) as u32).to_le_bytes());
        FrameHeader::decode(&just_too_long).unwrap_err();
    }

    #[test]
    fn record_len_round_trip() {
        const P: usize = RECORD_PLAINTEXT_LEN;

        for plaintext_len in [
            1,
            28,
            P - 1,
            P,
            P + 1,
            2 * P,
            2 * P + 1,
            MAX_FRAME_PAYLOAD_LEN,
        ] {
            let payload_len = record_frame_len(plaintext_len);
            assert_eq!(
                record_plaintext_len(payload_len).unwrap(),
                plaintext_len,
                "plaintext_len {plaintext_len}"
            );
        }

        assert_eq!(record_count(1), 1);
        assert_eq!(record_count(P), 1);
        assert_eq!(record_count(P + 1), 2);
        assert_eq!(record_frame_len(P), RECORD_LEN);
    }

    #[test]
    fn record_len_rejects_non_canonical() {
        // Two records where one filled record would have carried the same plaintext.
        assert!(record_plaintext_len(RECORD_LEN + RECORD_TAG_LEN).is_err());
        // Under-filled first record of a two record split.
        assert!(record_plaintext_len(record_frame_len(RECORD_PLAINTEXT_LEN) + 1).is_err());

        assert!(record_plaintext_len(0).is_err());
        assert!(record_plaintext_len(RECORD_TAG_LEN).is_err());
        assert!(record_plaintext_len(MAX_FRAME_LEN + 1).is_err());
    }
}
