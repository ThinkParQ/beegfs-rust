//! Loopback tests covering the framing of both protocols end to end.

use super::identity::{Identity, IdentityStore};
use super::noise::StaticKeypair;
use super::outgoing::Pool;
use super::*;
use crate::bee_msg::{Msg, MsgId};
use crate::bee_serde::*;
use crate::conn::msg_dispatch::{DispatchRequest, Request};
use crate::run_state::{self, RunStateControl};
use crate::types::Uid;
use anyhow::Result;
use bee_serde_derive::BeeSerde;
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

/// A listener and a pool pointed at it.
struct Loopback {
    pool: Pool,
    addr: SocketAddr,
    _control: RunStateControl,
}

impl Loopback {
    /// Both sides on the same protocol, with the keys registered where they need to be.
    async fn new(protocol: Protocol) -> Self {
        Self::build(protocol, protocol, true, Echo).await
    }

    /// `register_client_key` off leaves the responder without the initiators key, which is how a
    /// peer that was never provisioned looks.
    async fn build(
        server: Protocol,
        client: Protocol,
        register_client_key: bool,
        dispatch: impl DispatchRequest,
    ) -> Self {
        let (run_state, _control) = run_state::new();

        let localhost = SocketAddr::from(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));

        let server_keys = Arc::new(StaticKeypair::generate().unwrap());
        let client_keys = Arc::new(StaticKeypair::generate().unwrap());

        let server_ids = Arc::new(IdentityStore::new());
        if register_client_key {
            server_ids.replace_all([(
                client_keys.public(),
                Identity {
                    name: "client".into(),
                    node_uid: None,
                },
            )]);
        }

        let client_ids = Arc::new(IdentityStore::new());
        client_ids.replace_all([(
            server_keys.public(),
            Identity {
                name: "server".into(),
                node_uid: Some(PEER),
            },
        )]);

        let addr = incoming::listen_tcp(
            localhost,
            dispatch,
            cfg(server, server_keys, server_ids),
            run_state.clone(),
        )
        .await
        .unwrap();

        let pool = Pool::new(
            Arc::new(UdpSocket::bind(localhost).await.unwrap()),
            2,
            cfg(client, client_keys, client_ids),
            false,
        );
        pool.replace_node_addrs(PEER, vec![addr]);

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

fn cfg(
    protocol: Protocol,
    keypair: Arc<StaticKeypair>,
    identities: Arc<IdentityStore>,
) -> Arc<ConnConfig> {
    Arc::new(ConnConfig {
        protocol,
        legacy_auth_required: false,
        auth_secret: None,
        // Only the authenticating protocols look at it, and `check` rejects a missing one there.
        keypair: protocol.needs_handshake().then_some(keypair),
        identities,
    })
}

const ALL_PROTOCOLS: [Protocol; 4] = [
    Protocol::Legacy,
    Protocol::Plain,
    Protocol::Authenticated,
    Protocol::Encrypted,
];

/// Sizes around the Noise record boundaries are included already so the encrypted protocol reuses
/// this list unchanged.
fn payload_lens() -> Vec<usize> {
    use crate::protocol::RECORD_PLAINTEXT_LEN as P;

    vec![0, 1, 1000, P - 1, P, P + 1, 2 * P, 2 * P + 1]
}

/// Every protocol must carry every size unchanged. For `Encrypted` the larger sizes span two and
/// three Noise records.
#[tokio::test]
async fn round_trip_all_protocols() {
    for protocol in ALL_PROTOCOLS {
        let lb = Loopback::new(protocol).await;

        for len in payload_lens() {
            let msg = TestMsg::of_len(len);
            assert_eq!(
                lb.request(&msg).await.unwrap(),
                msg,
                "{protocol:?} with a payload of {len} bytes"
            );
        }
    }
}

/// The stream is reused across requests, so framing must stay aligned message after message. For
/// `Encrypted` this is also the nonce lockstep check: the counters are implicit, so one mismatched
/// record would break every message after it.
#[tokio::test]
async fn stream_reuse_stays_in_sync() {
    for protocol in ALL_PROTOCOLS {
        let lb = Loopback::new(protocol).await;

        for len in [1, 1000, 1, 70000, 5, 140000, 2] {
            let msg = TestMsg::of_len(len);
            assert_eq!(
                lb.request(&msg).await.unwrap(),
                msg,
                "{protocol:?} with a payload of {len} bytes"
            );
        }
    }
}

/// `Pool::send` never reads a response, so it is the only path that exercises writing on its own.
#[tokio::test]
async fn send_without_response_all_protocols() {
    for protocol in ALL_PROTOCOLS {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let lb = Loopback::build(protocol, protocol, true, Sink(tx)).await;

        for len in [1, 1000, 70000, 140000] {
            let msg = TestMsg::of_len(len);
            lb.pool.send(PEER, &msg).await.unwrap();

            assert_eq!(
                rx.recv().await.unwrap(),
                msg,
                "{protocol:?} with a payload of {len} bytes"
            );
        }
    }
}

/// Nonsense from a peer must close that one stream and leave the listener serving everybody else.
#[tokio::test]
async fn garbage_does_not_wedge_the_listener() {
    for protocol in ALL_PROTOCOLS {
        let lb = Loopback::new(protocol).await;

        let mut raw = TcpStream::connect(lb.addr).await.unwrap();
        raw.write_all(&[0xab; 512]).await.unwrap();

        // The responder may answer a Reject first, but it must end up hanging up either way.
        let mut discard = Vec::new();
        let closed = tokio::time::timeout(Duration::from_secs(2), raw.read_to_end(&mut discard))
            .await
            .is_ok();
        assert!(closed, "{protocol:?}: the garbage stream was not closed");
        drop(raw);

        let msg = TestMsg::of_len(16);
        assert_eq!(lb.request(&msg).await.unwrap(), msg, "{protocol:?}");
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
    use crate::bee_msg::{Header, deserialize_header, deserialize_header_v2};
    use crate::conn::handshake::{ClientHello, Reject, ServerHello};
    use crate::protocol::{FRAME_MAGIC, FrameHeader, FrameType, record_plaintext_len};

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
    let mut hellos_decoded = 0usize;
    let mut rejects_decoded = 0usize;

    for i in 0..20_000 {
        for chunk in buf.chunks_mut(8) {
            chunk.copy_from_slice(&next().to_le_bytes()[..chunk.len()]);
        }

        // Every fourth round, make the frame header well formed so what follows it gets reached.
        if i % 4 == 0 {
            let ftype = [
                FrameType::ClientHello,
                FrameType::ServerHello,
                FrameType::Reject,
                FrameType::Message,
            ][(next() % 4) as usize];

            FrameHeader {
                ftype,
                protected: next() % 2 == 0,
                payload_len: (next() as u32 as usize) % (crate::protocol::MAX_FRAME_LEN + 1),
            }
            .encode(&mut buf)
            .unwrap();
        } else if i % 4 == 1 {
            // Magic only, so the fields behind it carry random values.
            buf[0..4].copy_from_slice(&FRAME_MAGIC.to_le_bytes());
        }

        // Same idea for the legacy header, whose prefix check would otherwise reject everything.
        if i % 3 == 0 {
            buf[8..16].copy_from_slice(&Header::MSG_PREFIX.to_le_bytes());
        }

        let len = (next() as usize) % (buf.len() + 1);
        let bytes = &buf[..len];

        if FrameHeader::decode(bytes).is_ok() {
            frames_decoded += 1;
        }
        let _ = record_plaintext_len(next() as u32 as usize);
        let _ = deserialize_header(bytes);
        let _ = deserialize_header_v2(bytes, len);
        let _ = ClientHello::prologue(bytes);
        let _ = Reject::decode(bytes);

        // The payload decoders check the length first, so also feed them exactly what they want.
        // ClientHello additionally validates a u32 reserved field, which random bytes would hit
        // about once in four billion, so half the rounds get it zeroed.
        let mut hello = buf;
        if i % 2 == 0 {
            hello[4..8].fill(0);
        }
        if ClientHello::decode(&hello[..ClientHello::PAYLOAD_LEN]).is_ok() {
            hellos_decoded += 1;
        }
        let _ = ServerHello::decode(&buf[..ServerHello::PAYLOAD_LEN]);

        // Reject carries its own detail length, so half the rounds get one that fits the slice and
        // half keep the random value to exercise the rejection.
        let mut reject = buf;
        if i % 2 == 1 {
            reject[4..8].copy_from_slice(&((next() % 32) as u32).to_le_bytes());
        }
        if Reject::decode(&reject[..8 + 32]).is_ok() {
            rejects_decoded += 1;
        }
    }

    // Guards against the loop silently degenerating into "everything fails at byte 0".
    assert!(frames_decoded > 0, "no frame header ever decoded");
    assert!(hellos_decoded > 0, "no ClientHello ever decoded");
    assert!(rejects_decoded > 0, "no Reject ever decoded");
}

/// An unprovisioned peer must be refused, and the initiator must learn why.
#[tokio::test]
async fn unregistered_key_is_rejected() {
    for protocol in [Protocol::Authenticated, Protocol::Encrypted] {
        let lb = Loopback::build(protocol, protocol, false, Echo).await;

        let err = format!("{:#}", lb.request(&TestMsg::of_len(8)).await.unwrap_err());
        assert!(err.contains("not registered"), "{protocol:?}: {err}");
    }
}

/// The requested protection is bound into the prologue, so the two sides cannot end up disagreeing
/// about whether traffic is encrypted.
#[tokio::test]
async fn protection_mismatch_is_rejected() {
    for (server, client) in [
        (Protocol::Authenticated, Protocol::Encrypted),
        (Protocol::Encrypted, Protocol::Authenticated),
    ] {
        let lb = Loopback::build(server, client, true, Echo).await;

        let err = format!("{:#}", lb.request(&TestMsg::of_len(8)).await.unwrap_err());
        assert!(
            err.contains("not permitted") || err.contains("modes"),
            "server {server:?} / client {client:?}: {err}"
        );
    }
}

/// A protocol mismatch is a configuration error. It must surface as an error on both sides rather
/// than a panic or a hang.
#[tokio::test]
async fn protocol_mismatch_fails() {
    for (server, client) in [
        (Protocol::Legacy, Protocol::Plain),
        (Protocol::Plain, Protocol::Legacy),
    ] {
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
    let msg = TestMsg::of_len(8);
    let mut buf = [0u8; 128];

    // New protocol bytes arriving at a legacy reader.
    let len = crate::bee_msg::serialize(&msg, Protocol::Plain, &mut buf).unwrap();
    let err = format!(
        "{:#}",
        crate::bee_msg::deserialize_header(&buf[..len]).unwrap_err()
    );
    assert!(err.contains("Invalid BeeMsg prefix"), "{err}");

    // Legacy bytes arriving at a new protocol reader.
    let len = crate::bee_msg::serialize(&msg, Protocol::Legacy, &mut buf).unwrap();
    let err = format!(
        "{:#}",
        crate::protocol::FrameHeader::decode(&buf[..len]).unwrap_err()
    );
    assert!(err.contains("different BeeMsg protocol"), "{err}");
}

/// The legacy secret has no meaning in the new protocol, so the combination must not silently
/// downgrade authentication.
#[test]
fn conn_config_rejects_legacy_auth_with_new_protocol() {
    let with_secret = |protocol| ConnConfig {
        protocol,
        legacy_auth_required: true,
        auth_secret: Some(crate::types::AuthSecret::hash_from_bytes("secret")),
        keypair: None,
        identities: Arc::new(IdentityStore::new()),
    };

    with_secret(Protocol::Legacy).check().unwrap();
    with_secret(Protocol::Plain).check().unwrap_err();
    with_secret(Protocol::Authenticated).check().unwrap_err();
    with_secret(Protocol::Encrypted).check().unwrap_err();
}

/// The authenticating protocols cannot work without a keypair, so that must fail at startup rather
/// than at the first connection.
#[test]
fn conn_config_requires_a_keypair_for_the_handshake() {
    let without_keypair = |protocol| ConnConfig {
        protocol,
        legacy_auth_required: false,
        auth_secret: None,
        keypair: None,
        identities: Arc::new(IdentityStore::new()),
    };

    without_keypair(Protocol::Legacy).check().unwrap();
    without_keypair(Protocol::Plain).check().unwrap();
    without_keypair(Protocol::Authenticated)
        .check()
        .unwrap_err();
    without_keypair(Protocol::Encrypted).check().unwrap_err();
}
