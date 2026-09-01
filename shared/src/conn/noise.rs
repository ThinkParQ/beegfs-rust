//! Noise KK key exchange and record protection, a thin wrapper around the snow crate.
//!
//! KK means both peers hold a long term ("static") keypair and know the other's public key in
//! advance, so one round trip yields a mutually authenticated, forward secret session. The proof of
//! identity is the `ss` Diffie-Hellman term: only the two holders of those static private keys can
//! compute it, so there is no signature to verify.

use crate::protocol::{FRAME_HEADER_LEN, RECORD_LEN, RECORD_PLAINTEXT_LEN};
use crate::types::{StaticPubKey, parse_hex_32};
use anyhow::{Context, Result, anyhow, ensure};
use snow::params::DHChoice;
use snow::resolvers::{CryptoResolver, DefaultResolver};
use snow::{Builder, HandshakeState, TransportState};
use std::path::Path;
use zeroize::Zeroizing;

/// Identifies the construction. Changing it is a wire break and must come with a new
/// [`HS_VERSION`].
pub const PATTERN: &str = "Noise_KK_25519_ChaChaPoly_SHA256";

/// Wire version of the key exchange, sent in the clear in the first message.
pub const HS_VERSION: u16 = 1;

/// KK message 1 is the ephemeral public key plus the tag over an empty payload.
pub const NOISE_MSG_1_LEN: usize = 48;
/// KK message 2 is the ephemeral public key plus the tag over the 2 byte `agreed_modes` payload.
pub const NOISE_MSG_2_LEN: usize = 50;

const SECRET_LEN: usize = StaticPubKey::LEN;

/// `fixed_ephemeral` makes a handshake reproducible and is only ever `Some` in tests, where it is
/// what allows pinning cross tree vectors.
fn handshake_state(
    local: &StaticKeypair,
    peer: StaticPubKey,
    prologue: &[u8],
    initiator: bool,
    fixed_ephemeral: Option<&[u8]>,
) -> Result<HandshakeState> {
    let peer = *peer.as_bytes();

    let builder = Builder::new(PATTERN.parse().context("Invalid Noise pattern")?)
        .local_private_key(local.secret.as_ref())
        .map_err(|err| anyhow!("Rejected the local private key: {err}"))?
        .remote_public_key(&peer)
        .map_err(|err| anyhow!("Rejected the peer public key: {err}"))?
        .prologue(prologue)
        .map_err(|err| anyhow!("Rejected the handshake prologue: {err}"))?;

    let builder = match fixed_ephemeral {
        // `#[doc(hidden)]` in snow, so outside its documented surface - acceptable for a path only
        // tests reach.
        Some(ephemeral) => builder.fixed_ephemeral_key_for_testing_only(ephemeral),
        None => builder,
    };

    if initiator {
        builder.build_initiator()
    } else {
        builder.build_responder()
    }
    .map_err(|err| anyhow!("Building the Noise handshake state failed: {err}"))
}

/// A nodes long term keypair.
pub struct StaticKeypair {
    secret: Zeroizing<[u8; SECRET_LEN]>,
    public: StaticPubKey,
}

impl StaticKeypair {
    pub fn generate() -> Result<Self> {
        let pair = Builder::new(PATTERN.parse().context("Invalid Noise pattern")?)
            .generate_keypair()
            .map_err(|err| anyhow!("Generating a keypair failed: {err}"))?;

        Self::from_secret_bytes(pair.private.as_slice().try_into()?)
    }

    pub fn from_secret_bytes(secret: [u8; SECRET_LEN]) -> Result<Self> {
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

    /// Reads a key file written by `--gen-key`.
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read(path)
            .with_context(|| format!("Could not read the BeeMsg key file {path:?}"))?;

        Self::from_file_content(&raw).with_context(|| format!("Invalid BeeMsg key file {path:?}"))
    }

    /// Hex is what we write, raw bytes are accepted so an externally generated key can be dropped
    /// in unchanged.
    fn from_file_content(raw: &[u8]) -> Result<Self> {
        if let Ok(text) = std::str::from_utf8(raw)
            && text.trim().len() == SECRET_LEN * 2
        {
            return Self::from_secret_bytes(parse_hex_32(text)?);
        }

        ensure!(
            raw.len() == SECRET_LEN,
            "Expected {} raw bytes or {} hex characters, got {} bytes",
            SECRET_LEN,
            SECRET_LEN * 2,
            raw.len()
        );

        Self::from_secret_bytes(raw.try_into()?)
    }

    pub fn public(&self) -> StaticPubKey {
        self.public
    }

    pub fn secret_bytes(&self) -> Zeroizing<[u8; SECRET_LEN]> {
        self.secret.clone()
    }
}

/// Deliberately opaque: key material must never reach a log line.
impl std::fmt::Debug for StaticKeypair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "StaticKeypair({})", self.public)
    }
}

/// Initiator side, held between writing message 1 and reading message 2.
pub struct Initiator(HandshakeState);

impl Initiator {
    pub fn start(local: &StaticKeypair, peer: StaticPubKey, prologue: &[u8]) -> Result<Self> {
        Ok(Self(handshake_state(local, peer, prologue, true, None)?))
    }

    #[cfg(test)]
    pub(super) fn start_fixed(
        local: &StaticKeypair,
        peer: StaticPubKey,
        prologue: &[u8],
        ephemeral: &[u8],
    ) -> Result<Self> {
        Ok(Self(handshake_state(
            local,
            peer,
            prologue,
            true,
            Some(ephemeral),
        )?))
    }

    pub fn write_msg_1(&mut self, out: &mut [u8]) -> Result<usize> {
        self.0
            .write_message(&[], out)
            .map_err(|err| anyhow!("Writing key exchange message 1 failed: {err}"))
    }

    /// # Return value
    /// Returns the `agreed_modes` the responder put in the authenticated payload.
    pub fn read_msg_2(&mut self, msg: &[u8]) -> Result<u16> {
        let mut payload = [0u8; NOISE_MSG_2_LEN];

        let len = self
            .0
            .read_message(msg, &mut payload)
            .map_err(|err| anyhow!("Key exchange message 2 failed to authenticate: {err}"))?;

        ensure!(
            len == size_of::<u16>(),
            "Key exchange message 2 carries {len} payload bytes, expected {}",
            size_of::<u16>()
        );

        Ok(u16::from_le_bytes(payload[..2].try_into()?))
    }

    pub fn into_transport(self) -> Result<Transport> {
        Transport::new(self.0)
    }
}

/// Responder side, held between reading message 1 and writing message 2.
pub struct Responder(HandshakeState);

impl Responder {
    pub fn start(local: &StaticKeypair, peer: StaticPubKey, prologue: &[u8]) -> Result<Self> {
        Ok(Self(handshake_state(local, peer, prologue, false, None)?))
    }

    #[cfg(test)]
    pub(super) fn start_fixed(
        local: &StaticKeypair,
        peer: StaticPubKey,
        prologue: &[u8],
        ephemeral: &[u8],
    ) -> Result<Self> {
        Ok(Self(handshake_state(
            local,
            peer,
            prologue,
            false,
            Some(ephemeral),
        )?))
    }

    pub fn read_msg_1(&mut self, msg: &[u8]) -> Result<()> {
        let mut payload = [0u8; NOISE_MSG_1_LEN];

        let len = self
            .0
            .read_message(msg, &mut payload)
            .map_err(|err| anyhow!("Key exchange message 1 failed to authenticate: {err}"))?;

        ensure!(
            len == 0,
            "Key exchange message 1 carries {len} unexpected payload bytes"
        );

        Ok(())
    }

    pub fn write_msg_2(&mut self, agreed_modes: u16, out: &mut [u8]) -> Result<usize> {
        self.0
            .write_message(&agreed_modes.to_le_bytes(), out)
            .map_err(|err| anyhow!("Writing key exchange message 2 failed: {err}"))
    }

    pub fn into_transport(self) -> Result<Transport> {
        Transport::new(self.0)
    }
}

/// Post handshake record protection.
///
/// snow owns the per direction nonce counters and bumps one per Noise message. Since a record is
/// exactly one Noise message and TCP delivers them in order, both sides stay in lockstep without
/// any counter bookkeeping of ours.
pub struct Transport {
    state: Box<TransportState>,
    /// `[send prefix][record]`. The prefix lets the sender put the frame header in front of the
    /// first record and write both at once, see [`Transport::SEND_PREFIX_LEN`].
    scratch: Box<[u8; Transport::SEND_PREFIX_LEN + RECORD_LEN]>,
}

impl Transport {
    /// Bytes reserved in front of a sealed record.
    ///
    /// Records have to come out of a scratch buffer, so without this the frame header would need a
    /// write of its own - a write-write-read sequence that Nagle plus the peer's delayed ACK can
    /// stall by tens of milliseconds.
    pub const SEND_PREFIX_LEN: usize = FRAME_HEADER_LEN;

    fn new(handshake: HandshakeState) -> Result<Self> {
        Ok(Self {
            state: Box::new(
                handshake
                    .into_transport_mode()
                    .map_err(|err| anyhow!("Entering Noise transport mode failed: {err}"))?,
            ),
            scratch: Box::new([0u8; Transport::SEND_PREFIX_LEN + RECORD_LEN]),
        })
    }

    /// Encrypts one chunk of plaintext.
    ///
    /// # Return value
    /// Returns [`Transport::SEND_PREFIX_LEN`] writable bytes followed by the record, borrowed from
    /// the internal scratch buffer. Fill the prefix and write the whole slice, or skip it and write
    /// only the record.
    pub fn seal_record(&mut self, plaintext: &[u8]) -> Result<&mut [u8]> {
        ensure!(
            plaintext.len() <= RECORD_PLAINTEXT_LEN,
            "A record carries at most {RECORD_PLAINTEXT_LEN} plaintext bytes, got {}",
            plaintext.len()
        );

        let len = self
            .state
            .write_message(plaintext, &mut self.scratch[Self::SEND_PREFIX_LEN..])
            .map_err(|err| anyhow!("Sealing a record failed: {err}"))?;

        Ok(&mut self.scratch[..Self::SEND_PREFIX_LEN + len])
    }

    /// The scratch buffer slice to read the next record into.
    pub fn record_in(&mut self, len: usize) -> Result<&mut [u8]> {
        ensure!(
            len <= RECORD_LEN,
            "A record is at most {RECORD_LEN} bytes, peer announced {len}"
        );

        Ok(&mut self.scratch[Self::SEND_PREFIX_LEN..Self::SEND_PREFIX_LEN + len])
    }

    /// Decrypts and verifies what [`Transport::record_in`] was filled with into `out`.
    ///
    /// # Return value
    /// Returns the number of plaintext bytes written.
    pub fn open_record(&mut self, record_len: usize, out: &mut [u8]) -> Result<usize> {
        let record = self
            .scratch
            .get(Self::SEND_PREFIX_LEN..Self::SEND_PREFIX_LEN + record_len)
            .ok_or_else(|| anyhow!("A record is at most {RECORD_LEN} bytes, got {record_len}"))?;

        // Note the argument order: snow calls the ciphertext input `payload`.
        self.state
            .read_message(record, out)
            .map_err(|err| anyhow!("Opening a record failed: {err}"))
    }
}

/// Deliberately opaque: key material must never reach a log line.
impl std::fmt::Debug for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Transport(..)")
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::protocol::MODE_ENCRYPT;

    const PROLOGUE: &[u8] = b"prologue";

    /// The record without the send prefix, which is what actually goes on the wire behind the
    /// frame header.
    fn record(sealed: &[u8]) -> Vec<u8> {
        sealed[Transport::SEND_PREFIX_LEN..].to_vec()
    }

    fn pair() -> (StaticKeypair, StaticKeypair) {
        (
            StaticKeypair::generate().unwrap(),
            StaticKeypair::generate().unwrap(),
        )
    }

    /// Drives a full handshake and returns both transports, or the error that broke it.
    fn handshake(
        init: &StaticKeypair,
        init_peer: StaticPubKey,
        resp: &StaticKeypair,
        resp_peer: StaticPubKey,
        init_prologue: &[u8],
        resp_prologue: &[u8],
    ) -> Result<(Transport, Transport)> {
        let mut initiator = Initiator::start(init, init_peer, init_prologue)?;
        let mut responder = Responder::start(resp, resp_peer, resp_prologue)?;

        let mut msg1 = [0u8; NOISE_MSG_1_LEN];
        let len = initiator.write_msg_1(&mut msg1)?;
        assert_eq!(len, NOISE_MSG_1_LEN);
        responder.read_msg_1(&msg1)?;

        let mut msg2 = [0u8; NOISE_MSG_2_LEN];
        let len = responder.write_msg_2(MODE_ENCRYPT, &mut msg2)?;
        assert_eq!(len, NOISE_MSG_2_LEN);
        assert_eq!(initiator.read_msg_2(&msg2)?, MODE_ENCRYPT);

        Ok((initiator.into_transport()?, responder.into_transport()?))
    }

    /// The message lengths are wire constants and the pattern name is the construction identity.
    /// An accidental change to either is a silent interoperability break.
    #[test]
    fn suite_invariants() {
        assert_eq!(PATTERN, "Noise_KK_25519_ChaChaPoly_SHA256");
        PATTERN.parse::<snow::params::NoiseParams>().unwrap();

        let (a, b) = pair();
        handshake(&a, b.public(), &b, a.public(), PROLOGUE, PROLOGUE).unwrap();
    }

    #[test]
    fn round_trip() {
        let (a, b) = pair();
        let (mut init, mut resp) =
            handshake(&a, b.public(), &b, a.public(), PROLOGUE, PROLOGUE).unwrap();

        for len in [0, 1, 1000, RECORD_PLAINTEXT_LEN] {
            let plain: Vec<u8> = (0..len).map(|i| i as u8).collect();

            let record = record(init.seal_record(&plain).unwrap());
            assert_eq!(record.len(), plain.len() + crate::protocol::RECORD_TAG_LEN);
            if len > 0 {
                assert_ne!(record[..plain.len()], plain[..], "record is not encrypted");
            }

            resp.record_in(record.len())
                .unwrap()
                .copy_from_slice(&record);
            let mut out = vec![0u8; len];
            assert_eq!(resp.open_record(record.len(), &mut out).unwrap(), len);
            assert_eq!(out, plain);
        }

        assert!(
            init.seal_record(&vec![0u8; RECORD_PLAINTEXT_LEN + 1])
                .is_err()
        );
    }

    /// The send path relies on the frame header being contiguous with the first record, so that a
    /// message fitting one record goes out in a single write.
    #[test]
    fn sealed_record_reserves_the_frame_header() {
        assert_eq!(Transport::SEND_PREFIX_LEN, FRAME_HEADER_LEN);

        let (a, b) = pair();
        let (mut init, _) = handshake(&a, b.public(), &b, a.public(), PROLOGUE, PROLOGUE).unwrap();

        let out = init.seal_record(b"x").unwrap();
        assert_eq!(
            out.len(),
            Transport::SEND_PREFIX_LEN + 1 + crate::protocol::RECORD_TAG_LEN
        );

        // The reserved bytes are writable and sit in front of the record, not inside it.
        let record_before = out[Transport::SEND_PREFIX_LEN..].to_vec();
        out[..Transport::SEND_PREFIX_LEN].fill(0xaa);
        assert_eq!(&out[Transport::SEND_PREFIX_LEN..], &record_before[..]);
    }

    /// Keys and tags must be per session, otherwise a recorded session could be replayed.
    #[test]
    fn sessions_differ() {
        let (a, b) = pair();

        let mut first = handshake(&a, b.public(), &b, a.public(), PROLOGUE, PROLOGUE)
            .unwrap()
            .0;
        let mut second = handshake(&a, b.public(), &b, a.public(), PROLOGUE, PROLOGUE)
            .unwrap()
            .0;

        assert_ne!(
            record(first.seal_record(b"same plaintext").unwrap()),
            record(second.seal_record(b"same plaintext").unwrap())
        );
    }

    /// The whole point of KK: the handshake only completes if both sides hold the private key
    /// matching the public key the other one was configured with.
    #[test]
    fn wrong_keys_are_rejected() {
        let (a, b) = pair();
        let (stranger, _) = pair();

        // Responder expects somebody else.
        handshake(&a, b.public(), &b, stranger.public(), PROLOGUE, PROLOGUE).unwrap_err();
        // Initiator was given the wrong responder key.
        handshake(&a, stranger.public(), &b, a.public(), PROLOGUE, PROLOGUE).unwrap_err();
    }

    /// The prologue binds the cleartext hello into the transcript. Without this an on path attacker
    /// could flip the requested mode and silently downgrade an encrypted connection.
    #[test]
    fn prologue_mismatch_is_rejected() {
        let (a, b) = pair();

        handshake(&a, b.public(), &b, a.public(), PROLOGUE, b"tampered").unwrap_err();
    }

    #[test]
    fn tampered_record_is_rejected() {
        let (a, b) = pair();
        let (mut init, mut resp) =
            handshake(&a, b.public(), &b, a.public(), PROLOGUE, PROLOGUE).unwrap();

        let mut record = record(init.seal_record(b"payload").unwrap());
        let last = record.len() - 1;
        record[last] ^= 0xff;

        resp.record_in(record.len())
            .unwrap()
            .copy_from_slice(&record);
        resp.open_record(record.len(), &mut [0u8; 64]).unwrap_err();
    }

    /// Records carry no sequence number - the nonce is implicit - so a dropped or reordered record
    /// must fail rather than silently decrypt as a different one.
    #[test]
    fn skipped_record_is_rejected() {
        let (a, b) = pair();
        let (mut init, mut resp) =
            handshake(&a, b.public(), &b, a.public(), PROLOGUE, PROLOGUE).unwrap();

        let _skipped = record(init.seal_record(b"first").unwrap());
        let second = record(init.seal_record(b"second").unwrap());

        resp.record_in(second.len())
            .unwrap()
            .copy_from_slice(&second);
        resp.open_record(second.len(), &mut [0u8; 64]).unwrap_err();
    }

    #[test]
    fn keypair_round_trips_through_the_file_format() {
        let pair = StaticKeypair::generate().unwrap();

        let hex = format!("{}\n", hex_of(pair.secret_bytes().as_ref()));
        assert_eq!(
            StaticKeypair::from_file_content(hex.as_bytes())
                .unwrap()
                .public(),
            pair.public()
        );

        assert_eq!(
            StaticKeypair::from_file_content(pair.secret_bytes().as_ref())
                .unwrap()
                .public(),
            pair.public()
        );

        StaticKeypair::from_file_content(b"").unwrap_err();
        StaticKeypair::from_file_content(b"not hex and not 32 bytes long!!").unwrap_err();

        // The public half is safe to log, the private half must never show up.
        let debug = format!("{pair:?}");
        assert!(debug.contains(&pair.public().to_string()));
        assert!(!debug.contains(&hex_of(pair.secret_bytes().as_ref())));
    }

    fn hex_of(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
