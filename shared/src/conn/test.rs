//! Loopback tests covering the framing of both protocols end to end.

use super::outgoing::Pool;
use super::protocol::{
    ProtectedProtocol, Protocol, StaticKeypair, StaticPubKey, TransportProtectionMode,
};
use super::*;
use crate::bee_msg::{Msg, MsgId};
use crate::bee_serde::*;
use crate::conn::msg_dispatch::{DispatchRequest, Request};
use crate::conn::outgoing::PoolConfig;
use crate::run_state::{self, RunStateControl};
use crate::types::Uid;
use anyhow::Result;
use bee_serde_derive::BeeSerde;
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::mpsc;

const PEER: Uid = 1;

/// Variable length payload, so the same message covers all interesting sizes.
#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
struct TestMsg {
    #[bee_serde(as = CStr<0>)]
    payload: Vec<u8>,
}

impl Msg for TestMsg {
    // Not in NetMessageTypes.h - these messages never leave the test.
    const ID: MsgId = 65000;
}

impl TestMsg {
    fn of_len(len: usize) -> Self {
        Self {
            payload: (0..len).map(|i| i as u8).collect(),
        }
    }
}

#[derive(Clone, Debug)]
struct Echo;

impl DispatchRequest for Echo {
    async fn dispatch_request(&self, req: impl Request) -> Result<()> {
        let msg: TestMsg = req.deserialize_msg()?;
        req.respond(&msg).await
    }
}

/// Records what arrives and does not answer, which is what `Pool::send` expects.
#[derive(Clone, Debug)]
struct Sink(mpsc::UnboundedSender<TestMsg>);

impl DispatchRequest for Sink {
    async fn dispatch_request(&self, req: impl Request) -> Result<()> {
        let msg: TestMsg = req.deserialize_msg()?;
        let _ = self.0.send(msg);
        Ok(())
    }
}

/// Which protocol a test wants, without the key material.
///
/// [`Protocol`] carries this side's keypair, so [`Loopback`] builds the actual values - the two
/// sides need different keys, cross registered in each other's identity store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Legacy,
    Plain,
    Authenticated,
    Encrypted,
}

const ALL_KINDS: [Kind; 4] = [
    Kind::Legacy,
    Kind::Plain,
    Kind::Authenticated,
    Kind::Encrypted,
];

impl Kind {
    /// Binds this side's keypair.
    fn protocol(self, key_pair: StaticKeypair) -> Protocol {
        match self {
            // No secret: the legacy per message authentication is not what these tests cover.
            Self::Legacy => Protocol::Legacy(None),
            Self::Plain => Protocol::Plain,
            Self::Authenticated => Protocol::Protected(ProtectedProtocol::new(
                key_pair,
                TransportProtectionMode::Plain,
            )),
            Self::Encrypted => Protocol::Protected(ProtectedProtocol::new(
                key_pair,
                TransportProtectionMode::Encrypted,
            )),
        }
    }
}

/// Stands in for the management database. One per side, so the responder resolves the initiators
/// key and the initiator knows which key to expect and where to reach the peer.
#[derive(Clone, Debug, Default)]
struct TestLookup {
    identities: HashMap<StaticPubKey, Identity>,
    node_keys: HashMap<Uid, StaticPubKey>,
    node_addrs: HashMap<Uid, Vec<SocketAddr>>,
}

impl Lookup for TestLookup {
    async fn identity_by_key(&self, key: StaticPubKey) -> Result<Option<Identity>> {
        Ok(self.identities.get(&key).cloned())
    }

    async fn key_by_node(&self, node: Uid) -> Result<Option<StaticPubKey>> {
        Ok(self.node_keys.get(&node).copied())
    }

    async fn node_addrs(&self, node: Uid) -> Result<Option<Vec<SocketAddr>>> {
        Ok(self.node_addrs.get(&node).cloned())
    }
}

/// A listener and a pool pointed at it.
struct Loopback {
    pool: Pool<TestLookup>,
    addr: SocketAddr,
    _control: RunStateControl,
}

impl Loopback {
    /// Both sides on the same protocol, with the keys registered where they need to be.
    async fn new(kind: Kind) -> Self {
        Self::build(kind, kind, true, Echo).await
    }

    /// `register_client_key` off leaves the responder without the initiators key, which is how a
    /// peer that was never provisioned looks.
    async fn build(
        server: Kind,
        client: Kind,
        register_client_key: bool,
        dispatch: impl DispatchRequest,
    ) -> Self {
        let (run_state, _control) = run_state::new();

        let localhost = SocketAddr::from(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));

        let server_keys = StaticKeypair::generate().unwrap();
        let client_keys = StaticKeypair::generate().unwrap();

        // `server.protocol` consumes the keypair, so take the public half first.
        let server_keys_pub = server_keys.public();

        // The responder only needs to resolve the initiators key.
        let mut server_lookup = TestLookup::default();
        if register_client_key {
            server_lookup.identities.insert(
                client_keys.public(),
                Identity {
                    name: "client".into(),
                    node_uid: None,
                },
            );
        }

        let addr = incoming::listen_tcp(
            localhost,
            dispatch,
            server.protocol(server_keys),
            run_state.clone(),
            server_lookup,
        )
        .await
        .unwrap();

        // Built after `listen_tcp` because the listener port is ephemeral.
        let client_lookup = TestLookup {
            identities: HashMap::new(),
            node_keys: HashMap::from([(PEER, server_keys_pub)]),
            node_addrs: HashMap::from([(PEER, vec![addr])]),
        };

        let pool = Pool::new(
            client_lookup,
            Arc::new(UdpSocket::bind(localhost).await.unwrap()),
            PoolConfig {
                connection_limit: 2,
                protocol: client.protocol(client_keys),
                use_ipv6: false,
            },
        );

        Self {
            pool,
            addr,
            _control,
        }
    }

    async fn request(&self, msg: &TestMsg) -> Result<TestMsg> {
        let (resp, _) = self.pool.request(PEER, msg).await?;
        Ok(resp)
    }
}

/// Sizes around the Noise record boundaries are included already so the encrypted protocol reuses
/// this list unchanged.
fn payload_lens() -> Vec<usize> {
    use crate::conn::protocol::RECORD_PLAINTEXT_LEN as P;

    vec![0, 1, 1000, P - 1, P, P + 1, 2 * P, 2 * P + 1]
}

/// `Encrypted` must actually encrypt, and `Authenticated` must not.
///
/// Nothing ties a [`Kind`] to the [`Protection`] it passes on, so getting that pair wrong here
/// would silently run the whole encrypted matrix in the clear while every test still passed.
#[test]
fn kinds_request_the_protection_they_name() {
    // A fresh keypair per case - `StaticKeypair` is intentionally not `Clone`.
    for (kind, protects) in [(Kind::Authenticated, false), (Kind::Encrypted, true)] {
        let Protocol::Protected(p) = kind.protocol(StaticKeypair::generate().unwrap()) else {
            panic!("{kind:?} must be a protected protocol");
        };

        assert_eq!(p.protects_records(), protects, "{kind:?}");
    }

    for kind in [Kind::Legacy, Kind::Plain] {
        assert!(
            !matches!(
                kind.protocol(StaticKeypair::generate().unwrap()),
                Protocol::Protected(_)
            ),
            "{kind:?}"
        );
    }
}

/// Every protocol must carry every size unchanged. For `Encrypted` the larger sizes span two and
/// three Noise records.
#[tokio::test]
async fn round_trip_all_protocols() {
    for kind in ALL_KINDS {
        let lb = Loopback::new(kind).await;

        for len in payload_lens() {
            let msg = TestMsg::of_len(len);
            assert_eq!(
                lb.request(&msg).await.unwrap(),
                msg,
                "{kind:?} with a payload of {len} bytes"
            );
        }
    }
}

/// The stream is reused across requests, so framing must stay aligned message after message. For
/// `Encrypted` this is also the nonce lockstep check: the counters are implicit, so one mismatched
/// record would break every message after it.
#[tokio::test]
async fn stream_reuse_stays_in_sync() {
    for kind in ALL_KINDS {
        let lb = Loopback::new(kind).await;

        for len in [1, 1000, 1, 70000, 5, 140000, 2] {
            let msg = TestMsg::of_len(len);
            assert_eq!(
                lb.request(&msg).await.unwrap(),
                msg,
                "{kind:?} with a payload of {len} bytes"
            );
        }
    }
}

/// `Pool::send` never reads a response, so it is the only path that exercises writing on its own.
#[tokio::test]
async fn send_without_response_all_protocols() {
    for kind in ALL_KINDS {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let lb = Loopback::build(kind, kind, true, Sink(tx)).await;

        for len in [1, 1000, 70000, 140000] {
            let msg = TestMsg::of_len(len);
            lb.pool.send(PEER, &msg).await.unwrap();

            assert_eq!(
                rx.recv().await.unwrap(),
                msg,
                "{kind:?} with a payload of {len} bytes"
            );
        }
    }
}

/// Nonsense from a peer must close that one stream and leave the listener serving everybody else.
#[tokio::test]
async fn garbage_does_not_wedge_the_listener() {
    for kind in ALL_KINDS {
        let lb = Loopback::new(kind).await;

        let mut raw = TcpStream::connect(lb.addr).await.unwrap();
        raw.write_all(&[0xab; 512]).await.unwrap();

        // The responder may answer a HandshakeReject first, but it must end up hanging up either
        // way.
        let mut discard = Vec::new();
        let closed = tokio::time::timeout(Duration::from_secs(2), raw.read_to_end(&mut discard))
            .await
            .is_ok();
        assert!(closed, "{kind:?}: the garbage stream was not closed");
        drop(raw);

        let msg = TestMsg::of_len(16);
        assert_eq!(lb.request(&msg).await.unwrap(), msg, "{kind:?}");
    }
}

/// Every decoder that parses peer bytes must reject garbage rather than panic - a panic here fails
/// the test, which is the whole point.
///
/// Purely random bytes would be rejected by the first check almost every time, so some iterations
/// hand over a well formed prefix and the exact expected length. That is what gets the length and
/// field checks behind the cheap ones exercised.
#[test]
fn decoders_survive_random_input() {
    use crate::bee_msg::Header;
    use crate::conn::protocol::{
        FrameHeader, FrameType, MAX_FRAME_LEN, TransportProtectionMode, record_plaintext_len,
    };

    let mut state = 0x243f_6a88_85a3_08d3u64;
    let mut next = move || {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    };

    let mut buf = [0u8; 256];
    let mut frames_decoded = 0usize;
    let mut legacy_decoded = 0usize;

    for i in 0..20_000 {
        for chunk in buf.chunks_mut(8) {
            chunk.copy_from_slice(&next().to_le_bytes()[..chunk.len()]);
        }

        // Every fourth round, make the frame header well formed so what follows it gets reached.
        if i % 4 == 0 {
            let ftype = [
                FrameType::HandshakeInit,
                FrameType::HandshakeResponse,
                FrameType::HandshakeReject,
                FrameType::Message,
            ][(next() % 4) as usize];

            FrameHeader {
                frame_type: ftype,
                protection: if next() % 2 == 0 {
                    TransportProtectionMode::Plain
                } else {
                    TransportProtectionMode::Encrypted
                },
                frame_len: (next() as u32 as usize) % (MAX_FRAME_LEN + 1),
            }
            .serialize(&mut buf)
            .unwrap();
        } else if i % 4 == 1 {
            // Magic only, so the fields behind it carry random values.
            buf[0..4].copy_from_slice(&FrameHeader::MAGIC.to_le_bytes());
        }

        let len = (next() as usize) % (buf.len() + 1);
        let bytes = &buf[..len];

        if FrameHeader::deserialize(bytes).is_ok() {
            frames_decoded += 1;
        }
        let _ = record_plaintext_len(next() as u32 as usize);
        // The legacy header checks the prefix and then the length against the whole slice, so
        // without a valid magic and a fitting `msg_len` every round dies on the first field.
        let mut legacy = buf;
        if i % 3 == 0 && len >= Header::END_POS {
            let msg_len = Header::END_POS + (next() as usize) % (len - Header::END_POS + 1);
            legacy[0..4].copy_from_slice(&(msg_len as u32).to_le_bytes());
            legacy[8..16].copy_from_slice(&Header::LEGACY_MAGIC.to_le_bytes());
        }
        if Header::deserialize_legacy(&legacy[..len]).is_ok() {
            legacy_decoded += 1;
        }
        let _ = Header::deserialize(bytes, len);
    }

    // Guards against the loop silently degenerating into "everything fails at byte 0".
    assert!(frames_decoded > 0, "no frame header ever decoded");
    assert!(legacy_decoded > 0, "no legacy header ever decoded");
}

/// An unprovisioned peer must be refused, and the initiator must learn that it was.
#[tokio::test]
async fn unregistered_key_is_rejected() {
    for kind in [Kind::Authenticated, Kind::Encrypted] {
        let lb = Loopback::build(kind, kind, false, Echo).await;

        let err = format!("{:#}", lb.request(&TestMsg::of_len(8)).await.unwrap_err());
        assert!(err.contains("rejected the key exchange"), "{kind:?}: {err}");
        assert!(err.contains("UnknownIdentity"), "{kind:?}: {err}");
    }
}

/// The requested protection is bound into the prologue, so the two sides cannot end up disagreeing
/// about whether traffic is encrypted.
#[tokio::test]
async fn protection_mismatch_is_rejected() {
    for (server, client) in [
        (Kind::Authenticated, Kind::Encrypted),
        (Kind::Encrypted, Kind::Authenticated),
    ] {
        let lb = Loopback::build(server, client, true, Echo).await;

        let err = format!("{:#}", lb.request(&TestMsg::of_len(8)).await.unwrap_err());
        assert!(
            err.contains("rejected the key exchange"),
            "server {server:?} / client {client:?}: {err}"
        );
        assert!(
            err.contains("ModeNotPermitted"),
            "server {server:?} / client {client:?}: {err}"
        );
    }
}

/// A protocol mismatch is a configuration error. It must surface as an error on both sides rather
/// than a panic or a hang.
#[tokio::test]
async fn protocol_mismatch_fails() {
    for (server, client) in [(Kind::Legacy, Kind::Plain), (Kind::Plain, Kind::Legacy)] {
        let lb = Loopback::build(server, client, true, Echo).await;
        lb.request(&TestMsg::of_len(8))
            .await
            .expect_err(&format!("server {server:?} / client {client:?}"));
    }
}

/// The framing layer is where a mismatch is actually diagnosed. The initiator only ever sees the
/// peer hang up, so the message the responder logs has to name the likely cause.
#[test]
fn protocol_mismatch_is_diagnosed() {
    use crate::bee_msg::{Header, serialize_body};
    use crate::conn::protocol::{FrameHeader, FrameType, TransportProtectionMode};

    let msg = TestMsg::of_len(8);
    let mut buf = [0u8; 128];

    // New protocol bytes arriving at a legacy reader.
    let header = serialize_body(&msg, &mut buf).unwrap();
    FrameHeader {
        frame_type: FrameType::Message,
        protection: TransportProtectionMode::Plain,
        frame_len: header.msg_len(),
    }
    .serialize(&mut buf)
    .unwrap();
    header
        .serialize(&mut buf[FrameHeader::END_POS..Header::END_POS])
        .unwrap();
    let err = format!("{:#}", Header::deserialize_legacy(&buf).unwrap_err());
    assert!(err.contains("Invalid BeeMsg prefix"), "{err}");

    // Legacy bytes arriving at a new protocol reader.
    let header = serialize_body(&msg, &mut buf).unwrap();
    header
        .serialize_legacy(&mut buf[..Header::END_POS])
        .unwrap();
    let err = format!(
        "{:#}",
        FrameHeader::deserialize(&buf[..Header::END_POS]).unwrap_err()
    );
    assert!(err.contains("different BeeMsg protocol"), "{err}");
}
