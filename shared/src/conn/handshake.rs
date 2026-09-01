//! Drives the BeeMsg key exchange over a [`Stream`].
//!
//! Lives in the connection layer rather than the message dispatcher: the exchange completes before
//! the first BeeMsg is read, so no message id is spent on it, no handler sees it, and the
//! per message authentication check of the legacy protocol needs no counterpart here.

use super::identity::Identity;
use super::noise::{HS_VERSION, Initiator, NOISE_MSG_1_LEN, NOISE_MSG_2_LEN, Responder};
use super::stream::{MAX_CONTROL_PAYLOAD_LEN, Stream};
use super::{ConnConfig, GENERIC_STREAM_TIME_LIMIT};
use crate::protocol::FrameType;
use crate::types::StaticPubKey;
use anyhow::{Result, anyhow, bail, ensure};

/// The identity a completed key exchange proved.
#[derive(Clone, Debug)]
pub struct AuthenticatedPeer {
    pub static_pub: StaticPubKey,
    pub identity: Identity,
}

/// Why a responder refused. A stable numeric enum on the wire - append only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum RejectReason {
    Unspecified = 0,
    UnknownIdentity = 1,
    UnsupportedVersion = 2,
    ModeNotPermitted = 3,
    CryptoFailure = 4,
    ProtocolDisabled = 5,
}

impl RejectReason {
    /// A closed set of strings. Never echo anything derived from what the peer sent.
    fn detail(self) -> &'static str {
        match self {
            Self::Unspecified => "key exchange refused",
            Self::UnknownIdentity => "public key is not registered",
            Self::UnsupportedVersion => "unsupported key exchange version",
            Self::ModeNotPermitted => "requested protection is not permitted",
            Self::CryptoFailure => "key exchange failed to authenticate",
            Self::ProtocolDisabled => "the new BeeMsg protocol is disabled on this node",
        }
    }

    fn from_wire(value: u16) -> Self {
        match value {
            1 => Self::UnknownIdentity,
            2 => Self::UnsupportedVersion,
            3 => Self::ModeNotPermitted,
            4 => Self::CryptoFailure,
            5 => Self::ProtocolDisabled,
            _ => Self::Unspecified,
        }
    }
}

/// First frame, sent by the side opening the connection.
///
/// Travels in the clear - it holds no secrets, only public keys. `init_static_pub` selects which
/// key the responder authenticates us against, which KK needs before it can process `noise_msg_1`.
#[derive(Debug)]
pub struct ClientHello {
    pub hs_version: u16,
    pub modes: u16,
    pub init_static_pub: StaticPubKey,
    pub noise_msg_1: [u8; NOISE_MSG_1_LEN],
}

impl ClientHello {
    /// Everything before `noise_msg_1`, which is exactly the Noise prologue.
    pub const PROLOGUE_LEN: usize = 40;
    pub const PAYLOAD_LEN: usize = Self::PROLOGUE_LEN + NOISE_MSG_1_LEN;

    /// Writes the part covered by the prologue. Split out because the prologue has to be the exact
    /// bytes that go on the wire, so it must exist before the handshake state does.
    fn encode_prologue(buf: &mut [u8], modes: u16, init_static_pub: StaticPubKey) {
        buf[0..2].copy_from_slice(&HS_VERSION.to_le_bytes());
        buf[2..4].copy_from_slice(&modes.to_le_bytes());
        buf[4..8].copy_from_slice(&0u32.to_le_bytes());
        buf[8..40].copy_from_slice(init_static_pub.as_bytes());
    }

    /// The bytes both sides must mix into the transcript.
    ///
    /// This is what stops an on path attacker from flipping `modes` to downgrade an encrypted
    /// connection to an authenticated one.
    pub fn prologue(payload: &[u8]) -> Result<&[u8]> {
        payload
            .get(..Self::PROLOGUE_LEN)
            .ok_or_else(|| anyhow!("ClientHello is too short to contain a prologue"))
    }

    fn decode(payload: &[u8]) -> Result<Self> {
        ensure!(
            payload.len() == Self::PAYLOAD_LEN,
            "A ClientHello is {} bytes, got {}",
            Self::PAYLOAD_LEN,
            payload.len()
        );

        let reserved = u32::from_le_bytes(payload[4..8].try_into()?);
        ensure!(reserved == 0, "ClientHello reserved field is {reserved}");

        Ok(Self {
            hs_version: u16::from_le_bytes(payload[0..2].try_into()?),
            modes: u16::from_le_bytes(payload[2..4].try_into()?),
            init_static_pub: payload[8..40].try_into()?,
            noise_msg_1: payload[Self::PROLOGUE_LEN..].try_into()?,
        })
    }
}

/// Second frame. The last one in the clear - the `agreed_modes` it carries rides inside the Noise
/// payload, so it is authenticated.
#[derive(Debug)]
pub struct ServerHello {
    pub hs_version: u16,
    pub noise_msg_2: [u8; NOISE_MSG_2_LEN],
}

impl ServerHello {
    pub const PAYLOAD_LEN: usize = 8 + NOISE_MSG_2_LEN;

    fn encode(&self, buf: &mut [u8]) {
        buf[0..2].copy_from_slice(&self.hs_version.to_le_bytes());
        buf[2..8].fill(0);
        buf[8..].copy_from_slice(&self.noise_msg_2);
    }

    fn decode(payload: &[u8]) -> Result<Self> {
        ensure!(
            payload.len() == Self::PAYLOAD_LEN,
            "A ServerHello is {} bytes, got {}",
            Self::PAYLOAD_LEN,
            payload.len()
        );

        Ok(Self {
            hs_version: u16::from_le_bytes(payload[0..2].try_into()?),
            noise_msg_2: payload[8..].try_into()?,
        })
    }
}

/// Sent instead of a [`ServerHello`], after which the responder closes the connection.
#[derive(Debug)]
pub struct Reject {
    pub reason: RejectReason,
    pub detail: String,
}

impl Reject {
    const HEADER_LEN: usize = 8;
    const MAX_DETAIL_LEN: usize = 256;

    fn encode(reason: RejectReason, buf: &mut [u8]) -> usize {
        let detail = reason.detail().as_bytes();

        buf[0..2].copy_from_slice(&HS_VERSION.to_le_bytes());
        buf[2..4].copy_from_slice(&(reason as u16).to_le_bytes());
        buf[4..8].copy_from_slice(&(detail.len() as u32).to_le_bytes());
        buf[Self::HEADER_LEN..Self::HEADER_LEN + detail.len()].copy_from_slice(detail);

        Self::HEADER_LEN + detail.len()
    }

    fn decode(payload: &[u8]) -> Result<Self> {
        ensure!(
            payload.len() >= Self::HEADER_LEN,
            "A Reject is at least {} bytes, got {}",
            Self::HEADER_LEN,
            payload.len()
        );

        let detail_len = u32::from_le_bytes(payload[4..8].try_into()?) as usize;
        ensure!(
            detail_len <= Self::MAX_DETAIL_LEN,
            "Reject detail of {detail_len} bytes exceeds the maximum of {}",
            Self::MAX_DETAIL_LEN
        );

        let detail = payload
            .get(Self::HEADER_LEN..Self::HEADER_LEN + detail_len)
            .ok_or_else(|| anyhow!("Reject detail is truncated"))?;

        Ok(Self {
            reason: RejectReason::from_wire(u16::from_le_bytes(payload[2..4].try_into()?)),
            detail: String::from_utf8_lossy(detail).into_owned(),
        })
    }
}

/// Runs the key exchange on a freshly connected stream and installs the negotiated protection.
///
/// # Return value
/// Returns the peer key that was authenticated, which is the one that was passed in.
pub(super) async fn initiate(
    stream: &mut Stream,
    cfg: &ConnConfig,
    peer: StaticPubKey,
) -> Result<StaticPubKey> {
    let keypair = cfg
        .keypair
        .as_ref()
        .ok_or_else(|| anyhow!("No BeeMsg keypair configured, cannot authenticate"))?;

    let modes = cfg.protocol.requested_modes();

    let mut payload = [0u8; ClientHello::PAYLOAD_LEN];
    ClientHello::encode_prologue(&mut payload, modes, keypair.public());

    let mut initiator = Initiator::start(keypair, peer, ClientHello::prologue(&payload)?)?;
    let len = initiator.write_msg_1(&mut payload[ClientHello::PROLOGUE_LEN..])?;
    debug_assert_eq!(len, NOISE_MSG_1_LEN);

    stream
        .write_control_frame(FrameType::ClientHello, &payload, GENERIC_STREAM_TIME_LIMIT)
        .await?;

    let mut buf = [0u8; MAX_CONTROL_PAYLOAD_LEN];
    let (ftype, len) = stream
        .read_control_frame(&mut buf, GENERIC_STREAM_TIME_LIMIT)
        .await?;

    match ftype {
        FrameType::ServerHello => {}
        FrameType::Reject => {
            let reject = Reject::decode(&buf[..len])?;
            bail!(
                "Peer rejected the key exchange: {} ({:?}). Our public key is {}",
                reject.detail,
                reject.reason,
                keypair.public()
            );
        }
        other => bail!("Expected a ServerHello, got {other:?}"),
    }

    let hello = ServerHello::decode(&buf[..len])?;
    ensure!(
        hello.hs_version == HS_VERSION,
        "Peer answered with key exchange version {}, we speak {HS_VERSION}",
        hello.hs_version
    );

    let agreed = initiator.read_msg_2(&hello.noise_msg_2)?;
    ensure!(
        agreed == modes,
        "Peer agreed to protection modes {agreed:#06x} but {modes:#06x} were requested"
    );

    if cfg.protocol.protects_records() {
        stream.install_transport(initiator.into_transport()?);
    }

    Ok(peer)
}

/// Answers a key exchange on an accepted stream. The identity lookup *is* the authentication
/// decision.
///
/// On refusal the peer is told why before the caller closes the connection, so the failure is
/// diagnosable on both ends.
pub(super) async fn respond(stream: &mut Stream, cfg: &ConnConfig) -> Result<AuthenticatedPeer> {
    match respond_inner(stream, cfg).await {
        Ok(peer) => {
            stream.set_peer(peer.clone());
            Ok(peer)
        }
        Err((reason, err)) => {
            let mut buf = [0u8; MAX_CONTROL_PAYLOAD_LEN];
            let len = Reject::encode(reason, &mut buf);

            if let Err(send_err) = stream
                .write_control_frame(FrameType::Reject, &buf[..len], GENERIC_STREAM_TIME_LIMIT)
                .await
            {
                log::debug!("Could not send the key exchange rejection: {send_err:#}");
            }

            Err(err.context(format!("Rejected the key exchange ({reason:?})")))
        }
    }
}

async fn respond_inner(
    stream: &mut Stream,
    cfg: &ConnConfig,
) -> std::result::Result<AuthenticatedPeer, (RejectReason, anyhow::Error)> {
    use RejectReason::*;

    let keypair = cfg.keypair.as_ref().ok_or_else(|| {
        (
            ProtocolDisabled,
            anyhow!("No BeeMsg keypair configured on this node"),
        )
    })?;

    let mut buf = [0u8; MAX_CONTROL_PAYLOAD_LEN];
    let (ftype, len) = stream
        .read_control_frame(&mut buf, GENERIC_STREAM_TIME_LIMIT)
        .await
        .map_err(|err| (Unspecified, err))?;

    if ftype != FrameType::ClientHello {
        return Err((
            Unspecified,
            anyhow!("Expected a ClientHello, got {ftype:?}"),
        ));
    }

    let payload = &buf[..len];
    let hello = ClientHello::decode(payload).map_err(|err| (Unspecified, err))?;

    if hello.hs_version != HS_VERSION {
        return Err((
            UnsupportedVersion,
            anyhow!(
                "Peer speaks key exchange version {}, we speak {HS_VERSION}",
                hello.hs_version
            ),
        ));
    }

    if hello.modes != cfg.protocol.requested_modes() {
        return Err((
            ModeNotPermitted,
            anyhow!(
                "Peer requested protection modes {:#06x}, this node is configured for {:#06x}",
                hello.modes,
                cfg.protocol.requested_modes()
            ),
        ));
    }

    let identity = cfg
        .identities
        .identity_by_key(&hello.init_static_pub)
        .ok_or_else(|| {
            (
                UnknownIdentity,
                anyhow!(
                    "Peer presented public key {}, which is not registered",
                    hello.init_static_pub
                ),
            )
        })?;

    let prologue = ClientHello::prologue(payload).map_err(|err| (Unspecified, err))?;
    let mut responder = Responder::start(keypair, hello.init_static_pub, prologue)
        .map_err(|err| (CryptoFailure, err))?;
    responder
        .read_msg_1(&hello.noise_msg_1)
        .map_err(|err| (CryptoFailure, err))?;

    let mut noise_msg_2 = [0u8; NOISE_MSG_2_LEN];
    responder
        .write_msg_2(hello.modes, &mut noise_msg_2)
        .map_err(|err| (CryptoFailure, err))?;

    let mut payload = [0u8; ServerHello::PAYLOAD_LEN];
    ServerHello {
        hs_version: HS_VERSION,
        noise_msg_2,
    }
    .encode(&mut payload);

    stream
        .write_control_frame(FrameType::ServerHello, &payload, GENERIC_STREAM_TIME_LIMIT)
        .await
        .map_err(|err| (Unspecified, err))?;

    // Only after the reply is out - it is still unprotected.
    if cfg.protocol.protects_records() {
        stream.install_transport(
            responder
                .into_transport()
                .map_err(|err| (CryptoFailure, err))?,
        );
    }

    Ok(AuthenticatedPeer {
        static_pub: hello.init_static_pub,
        identity,
    })
}
#[cfg(test)]
mod test {
    use super::*;
    use crate::conn::noise::{StaticKeypair, Transport};
    use crate::protocol::MODE_ENCRYPT;

    /// Cross tree wire vectors.
    ///
    /// The C++ servers and the kernel client must reimplement this exchange byte for byte, so all
    /// three trees embed the same hex. Everything is fixed here: both static keys, both ephemeral
    /// keys and the requested modes.
    ///
    /// The ephemeral public keys below match the ones the independent implementation on the
    /// `hackathon/auth` branch derived from the same scalars, which cross checks the X25519 side.
    #[test]
    fn wire_vectors() {
        let init = StaticKeypair::from_secret_bytes([0x01; 32]).unwrap();
        let resp = StaticKeypair::from_secret_bytes([0x02; 32]).unwrap();

        assert_eq!(
            init.public().to_string(),
            "a4e09292b651c278b9772c569f5fa9bb13d906b46ab68c9df9dc2b4409f8a209"
        );
        assert_eq!(
            resp.public().to_string(),
            "ce8d3ad1ccb633ec7b70c17814a5c76ecd029685050d344745ba05870e587d59"
        );

        let mut client_hello = [0u8; ClientHello::PAYLOAD_LEN];
        ClientHello::encode_prologue(&mut client_hello, MODE_ENCRYPT, init.public());

        let mut initiator = Initiator::start_fixed(
            &init,
            resp.public(),
            ClientHello::prologue(&client_hello).unwrap(),
            &[0x03u8; 32],
        )
        .unwrap();
        initiator
            .write_msg_1(&mut client_hello[ClientHello::PROLOGUE_LEN..])
            .unwrap();

        assert_eq!(
            hex(&client_hello),
            // hs_version, modes, reserved
            "0100010000000000\
             a4e09292b651c278b9772c569f5fa9bb13d906b46ab68c9df9dc2b4409f8a209\
             5dfedd3b6bd47f6fa28ee15d969d5bb0ea53774d488bdaf9df1c6e0124b3ef22\
             7b407cca059a3e7dafbca3dcf1e4f296"
        );

        let mut responder = Responder::start_fixed(
            &resp,
            init.public(),
            ClientHello::prologue(&client_hello).unwrap(),
            &[0x04u8; 32],
        )
        .unwrap();

        let hello = ClientHello::decode(&client_hello).unwrap();
        assert_eq!(hello.hs_version, HS_VERSION);
        assert_eq!(hello.modes, MODE_ENCRYPT);
        assert_eq!(hello.init_static_pub, init.public());
        responder.read_msg_1(&hello.noise_msg_1).unwrap();

        let mut noise_msg_2 = [0u8; NOISE_MSG_2_LEN];
        responder
            .write_msg_2(MODE_ENCRYPT, &mut noise_msg_2)
            .unwrap();

        let mut server_hello = [0u8; ServerHello::PAYLOAD_LEN];
        ServerHello {
            hs_version: HS_VERSION,
            noise_msg_2,
        }
        .encode(&mut server_hello);

        assert_eq!(
            hex(&server_hello),
            // hs_version, reserved
            "0100000000000000\
             ac01b2209e86354fb853237b5de0f4fab13c7fcbf433a61c019369617fecf10b\
             abc977b34d742620eed958cbc07786246910"
        );

        assert_eq!(
            initiator
                .read_msg_2(&ServerHello::decode(&server_hello).unwrap().noise_msg_2)
                .unwrap(),
            MODE_ENCRYPT
        );

        // First record of the initiator to responder direction.
        let mut transport = initiator.into_transport().unwrap();
        assert_eq!(
            hex(&transport.seal_record(b"BeeGFS").unwrap()[Transport::SEND_PREFIX_LEN..]),
            "a65d0e3ec569e1f907a24588e763da3a1f9c1952021a"
        );
    }

    #[test]
    fn payloads_round_trip() {
        let mut client_hello = [0u8; ClientHello::PAYLOAD_LEN];
        ClientHello::encode_prologue(&mut client_hello, MODE_ENCRYPT, [0x07; 32].into());
        client_hello[ClientHello::PROLOGUE_LEN..].fill(0xab);

        let decoded = ClientHello::decode(&client_hello).unwrap();
        assert_eq!(decoded.hs_version, HS_VERSION);
        assert_eq!(decoded.modes, MODE_ENCRYPT);
        assert_eq!(decoded.init_static_pub, StaticPubKey::from([0x07; 32]));
        assert_eq!(decoded.noise_msg_1, [0xab; NOISE_MSG_1_LEN]);

        ClientHello::decode(&client_hello[..ClientHello::PAYLOAD_LEN - 1]).unwrap_err();
        ClientHello::prologue(&client_hello[..8]).unwrap_err();

        // Reserved fields must be rejected so they stay usable.
        let mut reserved = client_hello;
        reserved[4] = 1;
        ClientHello::decode(&reserved).unwrap_err();

        let mut server_hello = [0u8; ServerHello::PAYLOAD_LEN];
        ServerHello {
            hs_version: HS_VERSION,
            noise_msg_2: [0xcd; NOISE_MSG_2_LEN],
        }
        .encode(&mut server_hello);

        let decoded = ServerHello::decode(&server_hello).unwrap();
        assert_eq!(decoded.hs_version, HS_VERSION);
        assert_eq!(decoded.noise_msg_2, [0xcd; NOISE_MSG_2_LEN]);
        ServerHello::decode(&server_hello[..1]).unwrap_err();
    }

    #[test]
    fn reject_round_trips_every_reason() {
        for reason in [
            RejectReason::Unspecified,
            RejectReason::UnknownIdentity,
            RejectReason::UnsupportedVersion,
            RejectReason::ModeNotPermitted,
            RejectReason::CryptoFailure,
            RejectReason::ProtocolDisabled,
        ] {
            let mut buf = [0u8; 512];
            let len = Reject::encode(reason, &mut buf);

            let decoded = Reject::decode(&buf[..len]).unwrap();
            assert_eq!(decoded.reason, reason);
            assert_eq!(decoded.detail, reason.detail());
        }

        Reject::decode(&[0u8; 4]).unwrap_err();

        // A detail length beyond the frame must not be trusted.
        let mut truncated = [0u8; 8];
        truncated[4..8].copy_from_slice(&64u32.to_le_bytes());
        Reject::decode(&truncated).unwrap_err();
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
