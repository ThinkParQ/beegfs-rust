use super::TCP_BUF_LEN;
use super::noise::PATTERN;
use crate::bee_serde::{
    BeeSerdeConversion, Deserializable, Deserializer, Serializable, Serializer,
};
use crate::types::AuthSecret;
use anyhow::{Context, Result, anyhow, ensure};
use bee_serde_derive::BeeSerde;
use snow::Builder;
use snow::params::DHChoice;
use snow::resolvers::{CryptoResolver, DefaultResolver};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use zeroize::Zeroizing;

/// The BeeMsg wire format and protocol to use for connections
#[derive(Clone, Debug)]
pub enum Protocol {
    /// Old format with the `AuthenticateChannel` shared secret.
    Legacy(Option<AuthSecret>),
    /// New framing, no authentication and no encryption.
    Plain,
    /// New framing with Noise KK handshake and optional encryption/protection
    Protected(ProtectedProtocol),
}

impl Protocol {
    pub const fn is_legacy(&self) -> bool {
        matches!(self, Self::Legacy(_))
    }
}

#[derive(Clone, Debug)]
pub struct ProtectedProtocol {
    pub key_pair: Arc<StaticKeypair>,
    pub transport_protection: TransportProtectionMode,
}

impl ProtectedProtocol {
    pub fn new(key_pair: StaticKeypair, protection: TransportProtectionMode) -> Self {
        Self {
            key_pair: Arc::new(key_pair),
            transport_protection: protection,
        }
    }
}

/// On-wire size of the record authentication tag (snow's `TAGLEN`)
pub const RECORD_TAG_LEN: usize = 16;
/// On-wire size of a full record, the maximum allowed by noise
pub const RECORD_LEN: usize = 65535;
/// Plaintext carried by a full record.
pub const RECORD_PLAINTEXT_LEN: usize = RECORD_LEN - RECORD_TAG_LEN;

/// Largest plaintext a frame can carry: what is left of the stream buffer after the frame header.
pub const MAX_FRAME_PAYLOAD_LEN: usize = TCP_BUF_LEN - FrameHeader::END_POS;
/// Largest total frame length. Exceeds [`MAX_FRAME_PAYLOAD_LEN`] because the frame header and one
/// record tag per record are added to it.
pub const MAX_FRAME_LEN: usize = record_frame_len(MAX_FRAME_PAYLOAD_LEN);

/// Total frame length for `plaintext_len` bytes carried as Noise records.
pub const fn record_frame_len(plaintext_len: usize) -> usize {
    FrameHeader::END_POS
        + plaintext_len
        + plaintext_len.div_ceil(RECORD_PLAINTEXT_LEN) * RECORD_TAG_LEN
}

/// Inverse of [`record_frame_len`].
pub fn record_plaintext_len(frame_len: usize) -> Result<usize> {
    let payload_len = frame_len
        .checked_sub(FrameHeader::END_POS)
        .ok_or_else(|| anyhow!("{frame_len} is smaller than the frame header"))?;

    ensure!(payload_len > RECORD_TAG_LEN);

    let records = payload_len.div_ceil(RECORD_LEN);
    let plaintext_len = payload_len - records * RECORD_TAG_LEN;

    ensure!(plaintext_len > (records - 1) * RECORD_PLAINTEXT_LEN);
    ensure!(plaintext_len <= MAX_FRAME_PAYLOAD_LEN);

    Ok(plaintext_len)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameType {
    HandshakeInit,
    HandshakeResponse,
    HandshakeReject,
    Message,
}

impl_enum_bee_msg_traits!(FrameType,
    HandshakeInit => 1,
    HandshakeResponse => 2,
    HandshakeReject => 3,
    Message => 4
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportProtectionMode {
    Plain,
    Encrypted,
}

/// `modes` bit 0: protect the frame bodies with Noise records.
pub const MODE_ENCRYPT: u16 = 1 << 0;

impl TransportProtectionMode {
    /// The `modes` bit field this mode puts on the wire.
    pub const fn modes(self) -> u16 {
        match self {
            Self::Plain => 0,
            Self::Encrypted => MODE_ENCRYPT,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameHeader {
    pub frame_type: FrameType,
    pub protection: TransportProtectionMode,
    /// Total frame length, including frame header
    pub frame_len: usize,
}

impl Serializable for FrameHeader {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.u32(Self::MAGIC)?;
        ser.u32(self.frame_len.try_into()?)?;
        let frame_type = self.frame_type.into_bee_serde();
        ser.u8(frame_type)?;
        ser.u8(match self.protection {
            TransportProtectionMode::Plain => 0,
            TransportProtectionMode::Encrypted => Self::FLAG_NOISE_RECORDS,
        })?;
        ser.u16(0)?;

        Ok(())
    }
}

impl Deserializable for FrameHeader {
    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self>
    where
        Self: Sized,
    {
        let magic = des.u32()?;
        ensure!(
            magic == Self::MAGIC,
            "invalid frame magic {magic:#010x}, expected {:#010x}. \
The peer is most likely configured for a different BeeMsg protocol",
            Self::MAGIC
        );

        let frame_len: usize = des.u32()?.try_into()?;
        ensure!(
            frame_len >= Self::END_POS,
            "frame length {frame_len} is smaller than the frame header {}",
            Self::END_POS
        );
        ensure!(
            frame_len <= MAX_FRAME_LEN,
            "frame length {frame_len} exceeds the maximum of {MAX_FRAME_LEN}"
        );

        let frame_type = FrameType::try_from_bee_serde(des.u8()?)?;
        let flags = des.u8()?;
        ensure!(
            flags & Self::FLAGS_UNUSED == 0,
            "unused frame flag bits set ({flags:#04x})"
        );
        ensure!(
            flags & Self::FLAG_RESERVED == 0,
            "reserved frame flag bit set ({flags:#04x})"
        );
        let protection = if flags & Self::FLAG_NOISE_RECORDS != 0 {
            TransportProtectionMode::Encrypted
        } else {
            TransportProtectionMode::Plain
        };

        let reserved = des.u16()?;
        ensure!(reserved == 0, "reserved field is {reserved}, must be 0");

        Ok(Self {
            frame_type,
            protection,
            frame_len,
        })
    }
}

impl FrameHeader {
    /// The position in the on-wire data after the frame header ends and the frame body starts
    pub const END_POS: usize = 12;

    /// Identifies a BeeMsg frame. Serialized little endian, so it reads as "BGFS" on the wire.
    pub const MAGIC: u32 = 0x5346_4742;

    /// `frame_flags` bit 0: the body is a sequence of Noise records.
    const FLAG_NOISE_RECORDS: u8 = 1 << 0;
    /// Reserved for a future authenticate-without-encrypt mode
    const FLAG_RESERVED: u8 = 1 << 1;
    const FLAGS_UNUSED: u8 = !(Self::FLAG_NOISE_RECORDS | Self::FLAG_RESERVED);

    pub const fn body_len(&self) -> usize {
        self.frame_len.saturating_sub(Self::END_POS)
    }

    pub fn serialize(&self, buf: &mut [u8]) -> Result<()> {
        Serializable::serialize(self, &mut Serializer::new(buf))
    }

    pub fn deserialize(buf: &[u8]) -> Result<Self> {
        let mut des = Deserializer::new(buf);
        let frame = Deserializable::deserialize(&mut des)?;
        des.finish()?;

        Ok(frame)
    }
}

/// A nodes static X25519 public key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct StaticPubKey([u8; Self::LEN]);

impl StaticPubKey {
    pub const LEN: usize = 32;

    pub fn as_bytes(&self) -> &[u8; Self::LEN] {
        &self.0
    }
}

impl From<[u8; Self::LEN]> for StaticPubKey {
    fn from(bytes: [u8; Self::LEN]) -> Self {
        Self(bytes)
    }
}

impl TryFrom<&[u8]> for StaticPubKey {
    type Error = anyhow::Error;

    fn try_from(bytes: &[u8]) -> Result<Self> {
        Ok(Self(bytes.try_into().with_context(|| {
            format!(
                "A public key must be exactly {} bytes, got {}",
                Self::LEN,
                bytes.len()
            )
        })?))
    }
}

impl std::fmt::Display for StaticPubKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for StaticPubKey {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        Ok(Self(parse_hex_32(s)?))
    }
}

/// A nodes static X25519 keypair.
///
/// Not [`Clone`] to avoid having the secret key all over the memory. Instead, an `Arc` should be
/// used.
pub struct StaticKeypair {
    pub(super) secret: Zeroizing<[u8; Self::SECRET_LEN]>,
    public: StaticPubKey,
}

impl StaticKeypair {
    const SECRET_LEN: usize = StaticPubKey::LEN;

    pub fn generate() -> Result<Self> {
        let pair = Builder::new(PATTERN.parse().context("Invalid Noise pattern")?)
            .generate_keypair()
            .map_err(|err| anyhow!("Generating a keypair failed: {err}"))?;

        Self::from_secret_bytes(pair.private.as_slice().try_into()?)
    }

    pub fn from_secret_bytes(secret: [u8; Self::SECRET_LEN]) -> Result<Self> {
        // snow has no "public key from private key", so go through the DH primitive directly. The
        // ring resolver has no X25519, hence the default one.
        let mut dh = DefaultResolver
            .resolve_dh(&DHChoice::Curve25519)
            .ok_or_else(|| anyhow!("No X25519 implementation available"))?;
        dh.set(&secret);

        Ok(Self {
            secret: Zeroizing::new(secret),
            public: dh.pubkey().try_into()?,
        })
    }

    /// Reads a key file.
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read(path)
            .with_context(|| format!("Could not read the BeeMsg key file {path:?}"))?;

        Self::from_file_content(&raw).with_context(|| format!("Invalid BeeMsg key file {path:?}"))
    }

    pub(super) fn from_file_content(raw: &[u8]) -> Result<Self> {
        if let Ok(text) = std::str::from_utf8(raw)
            && text.trim().len() == Self::SECRET_LEN * 2
        {
            return Self::from_secret_bytes(parse_hex_32(text)?);
        }

        ensure!(
            raw.len() == Self::SECRET_LEN,
            "Expected {} raw bytes or {} hex characters, got {} bytes",
            Self::SECRET_LEN,
            Self::SECRET_LEN * 2,
            raw.len()
        );

        Self::from_secret_bytes(raw.try_into()?)
    }

    pub fn public(&self) -> StaticPubKey {
        self.public
    }

    pub fn secret_bytes(&self) -> Zeroizing<[u8; Self::SECRET_LEN]> {
        self.secret.clone()
    }
}

impl std::fmt::Debug for StaticKeypair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Keep private key out of logs
        write!(f, "StaticKeypair({})", self.public)
    }
}

/// Parses 64 hex characters into 32 bytes.
fn parse_hex_32(s: &str) -> Result<[u8; 32]> {
    let s = s.trim();
    anyhow::ensure!(
        s.len() == 64,
        "A hex encoded 32 byte key must be 64 characters, got {}",
        s.len()
    );

    let mut key = [0u8; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
            .context("Not a valid hex encoded 32 byte key")?;
    }

    Ok(key)
}
