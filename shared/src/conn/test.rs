//! End to end tests for the connection layer.
//!
//! These drive a real [`Pool`] against a real [`listen_tcp`] over loopback TCP, which is the only
//! way to cover the parts the unit tests cannot: that the two plaintext handshake messages are
//! framed correctly on the wire, that the session takes effect on the responder only *after* its
//! reply has been written (see [`Stream::stage_session`]), and that the message counters on both
//! sides then stay in lockstep.

use super::incoming::listen_tcp;
use super::key_store::KeyStore;
use super::outgoing::Pool;
use crate::bee_msg::misc::{Ack, KeyExchangeRequest, KeyExchangeResponse};
use crate::bee_msg::{
    Header, Msg, deserialize_body, deserialize_encryption_header, deserialize_header, serialize,
};
use crate::conn::msg_dispatch::{DispatchRequest, Request};
use crate::conn::stream::Stream;
use crate::crypto::handshake::{self, StaticKeypair};
use crate::run_state;
use crate::types::{AuthSecret, Uid};
use anyhow::{Result, bail};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use tokio::net::UdpSocket;

const PEER_UID: Uid = 4242;

/// The responder side: performs the key exchange, then echoes [`Ack`] messages back.
///
/// Mirrors what the management handler does, without pulling in a database.
#[derive(Clone, Debug)]
struct TestResponder {
    keypair: Arc<StaticKeypair>,
    key_store: Arc<KeyStore>,
    /// Reply to a key exchange with a default initialized response instead of failing the
    /// connection. This is what the real message dispatcher does when a handler errors, since a
    /// typed handler cannot substitute a `GenericResponse`.
    reply_with_default_on_reject: bool,
}

impl DispatchRequest for TestResponder {
    async fn dispatch_request(&self, req: impl Request) -> Result<()> {
        let mut req = req;

        match req.header().msg_id() {
            KeyExchangeRequest::ID => {
                let msg: KeyExchangeRequest = req.deserialize_msg()?;

                // The allow list lookup is the authentication decision.
                let Some(_uid) = self.key_store.node_by_key(&msg.static_pub) else {
                    if self.reply_with_default_on_reject {
                        return req.respond(&KeyExchangeResponse::default()).await;
                    }
                    bail!("unknown peer key {}", msg.static_pub);
                };

                let resp = handshake::respond(&self.keypair, msg.static_pub, &msg.ephemeral_pub)?;

                // Staged, so the plaintext reply below still goes out unencrypted.
                req.install_session(resp.session, msg.static_pub);

                req.respond(&KeyExchangeResponse {
                    ephemeral_pub: resp.ephemeral_pub,
                    confirm: resp.confirm,
                })
                .await
            }

            Ack::ID => {
                let msg: Ack = req.deserialize_msg()?;

                // Echo the payload back so the caller can verify it survived the round trip.
                req.respond(&Ack { ack_id: msg.ack_id }).await
            }

            id => bail!("unexpected msg id {id}"),
        }
    }
}

struct Fixture {
    pool: Pool,
    _run_state_control: run_state::RunStateControl,
}

/// Brings up a responder on an ephemeral loopback port and a pool configured to reach it.
///
/// `register_initiator_key` controls whether the responder has the initiator's public key on file -
/// setting it to false is how the rejection path is exercised. `reply_with_default_on_reject`
/// selects how the responder refuses: by dropping the connection, or by answering with a default
/// initialized response the way the real dispatcher does.
async fn setup_with(register_initiator_key: bool, reply_with_default_on_reject: bool) -> Fixture {
    let initiator_kp = Arc::new(StaticKeypair::generate());
    let responder_kp = Arc::new(StaticKeypair::generate());

    let (run_state, run_state_control) = run_state::new();

    // Both sides share one list here; in a real deployment each downloads it from management.
    let key_store = Arc::new(KeyStore::new());
    key_store.insert(PEER_UID, responder_kp.public());
    if register_initiator_key {
        key_store.insert(1, initiator_kp.public());
    }

    let listen_addr = listen_tcp(
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
        TestResponder {
            keypair: responder_kp,
            key_store: key_store.clone(),
            reply_with_default_on_reject,
        },
        // Authentication required, so this also covers the key exchange being allowed through on a
        // not yet authenticated stream.
        true,
        run_state,
    )
    .await
    .unwrap();

    let udp_socket = Arc::new(
        UdpSocket::bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)))
            .await
            .unwrap(),
    );

    let pool = Pool::new(
        udp_socket,
        4,
        Some(AuthSecret::hash_from_bytes("secret")),
        false,
        Some(initiator_kp),
        key_store,
    );
    pool.replace_node_addrs(PEER_UID, vec![listen_addr]);

    Fixture {
        pool,
        _run_state_control: run_state_control,
    }
}

async fn setup(register_initiator_key: bool) -> Fixture {
    setup_with(register_initiator_key, false).await
}

/// The full path: connect, key exchange, then a real encrypted request/response round trip.
#[tokio::test]
async fn handshake_then_encrypted_request() {
    let fixture = setup(true).await;

    let resp: Ack = fixture
        .pool
        .request(
            PEER_UID,
            &Ack {
                ack_id: b"first".to_vec(),
            },
        )
        .await
        .expect("request over the encrypted stream must succeed");

    assert_eq!(resp.ack_id, b"first");
}

/// The counters advance per message, so a second and third request on the *same* pooled stream use
/// nonces 1 and 2. This is what catches an off-by-one or a desynchronized counter - the first
/// message alone would pass even if the counters were broken, since both sides start at zero.
#[tokio::test]
async fn counters_stay_in_lockstep_across_messages() {
    let fixture = setup(true).await;

    for payload in [b"one".to_vec(), b"two".to_vec(), b"three".to_vec()] {
        let resp: Ack = fixture
            .pool
            .request(
                PEER_UID,
                &Ack {
                    ack_id: payload.clone(),
                },
            )
            .await
            .unwrap_or_else(|err| panic!("request with payload {payload:?} failed: {err:#}"));

        assert_eq!(resp.ack_id, payload);
    }
}

/// A responder that does not have our public key on file must refuse to establish a session, and
/// the initiator must surface that as an error rather than proceeding unencrypted.
#[tokio::test]
async fn unregistered_initiator_key_is_rejected() {
    let fixture = setup(false).await;

    let res: Result<Ack> = fixture
        .pool
        .request(
            PEER_UID,
            &Ack {
                ack_id: b"nope".to_vec(),
            },
        )
        .await;

    let err = res.expect_err("a peer that does not know our key must not get an encrypted session");
    let err = format!("{err:#}");
    assert!(
        err.contains("Key exchange") || err.contains("key exchange"),
        "error should name the key exchange, got: {err}"
    );
}

/// The real dispatcher cannot substitute a `GenericResponse` for a typed handler's response, so a
/// refused key exchange comes back as an all-zero `KeyExchangeResponse`. The initiator must name
/// the likely cause rather than let the zeroed ephemeral key surface as a low-order-point error.
#[tokio::test]
async fn zeroed_response_reports_unregistered_key() {
    let fixture = setup_with(false, true).await;

    let res: Result<Ack> = fixture
        .pool
        .request(
            PEER_UID,
            &Ack {
                ack_id: b"nope".to_vec(),
            },
        )
        .await;

    let err = format!(
        "{:#}",
        res.expect_err("a zeroed response must not yield a session")
    );
    assert!(
        err.contains("not registered"),
        "error should point at the unregistered key, got: {err}"
    );
    assert!(
        !err.contains("low order"),
        "the confusing low-order-point error must not leak through, got: {err}"
    );
}

/// Without a keypair the pool must not attempt a handshake at all - this is the
/// authentication-disabled configuration, where traffic stays plaintext.
#[tokio::test]
async fn no_keypair_skips_handshake() {
    let (run_state, _control) = run_state::new();
    let key_store = Arc::new(KeyStore::new());

    // A responder that only echoes; it would fail on a KeyExchangeRequest.
    let listen_addr = listen_tcp(
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
        TestResponder {
            keypair: Arc::new(StaticKeypair::generate()),
            key_store: key_store.clone(),
            reply_with_default_on_reject: false,
        },
        false,
        run_state,
    )
    .await
    .unwrap();

    let udp_socket = Arc::new(
        UdpSocket::bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)))
            .await
            .unwrap(),
    );

    let pool = Pool::new(udp_socket, 4, None, false, None, key_store);
    pool.replace_node_addrs(PEER_UID, vec![listen_addr]);

    let resp: Ack = pool
        .request(
            PEER_UID,
            &Ack {
                ack_id: b"plain".to_vec(),
            },
        )
        .await
        .expect("plaintext request must work when no keypair is configured");

    assert_eq!(resp.ack_id, b"plain");
}

/// Proves the traffic really is encrypted after the handshake, rather than both sides silently
/// agreeing to stay in the clear - which the round trip tests above could not distinguish.
///
/// Drives the initiator half by hand so the last message can deliberately be sent *unencrypted*.
/// A responder that has its session active cannot authenticate it, so it drops the connection.
#[tokio::test]
async fn responder_rejects_plaintext_after_handshake() {
    let initiator_kp = Arc::new(StaticKeypair::generate());
    let responder_kp = Arc::new(StaticKeypair::generate());

    let (run_state, _control) = run_state::new();
    let key_store = Arc::new(KeyStore::new());
    key_store.insert(1, initiator_kp.public());

    let listen_addr = listen_tcp(
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
        TestResponder {
            keypair: responder_kp.clone(),
            key_store: key_store.clone(),
            reply_with_default_on_reject: false,
        },
        true,
        run_state,
    )
    .await
    .unwrap();

    let mut stream = Stream::connect_tcp(&listen_addr).await.unwrap();
    let mut buf = vec![0u8; 64 * 1024];

    // --- handshake, by hand ---
    let initiator = handshake::Initiator::start(&initiator_kp, responder_kp.public()).unwrap();
    let len = serialize(
        &KeyExchangeRequest {
            version: handshake::HANDSHAKE_VERSION,
            static_pub: initiator_kp.public(),
            ephemeral_pub: initiator.ephemeral_pub(),
        },
        &mut buf,
    )
    .unwrap();
    stream.write_all(&buf[..len]).await.unwrap();

    stream.read_exact(&mut buf[..Header::LEN]).await.unwrap();
    let resp_len = deserialize_encryption_header(&buf[..Header::ENCRYPTION_INFO_LEN]).unwrap();
    stream
        .read_exact(&mut buf[Header::LEN..resp_len])
        .await
        .unwrap();
    let header = deserialize_header(&buf[..Header::LEN]).unwrap();
    let resp: KeyExchangeResponse = deserialize_body(&header, &buf[Header::LEN..]).unwrap();

    initiator
        .finish(&resp.ephemeral_pub, &resp.confirm)
        .expect("handshake must complete");

    // --- now misbehave: send the next message in the clear ---
    let len = serialize(
        &Ack {
            ack_id: b"plaintext-after-handshake".to_vec(),
        },
        &mut buf,
    )
    .unwrap();
    // Deliberately NOT encrypted. If the responder were also still in the clear, it would happily
    // parse this and answer.
    stream.write_all(&buf[..len]).await.unwrap();

    let res = stream.read_exact(&mut buf[..Header::LEN]).await;
    assert!(
        res.is_err(),
        "responder accepted an unencrypted message after the handshake, so the session was never \
         activated on its side"
    );

    // And confirm the same stream state *would* have worked when encrypted properly, so the
    // rejection above is really about encryption and not some unrelated framing problem.
    let mut stream = Stream::connect_tcp(&listen_addr).await.unwrap();
    let initiator = handshake::Initiator::start(&initiator_kp, responder_kp.public()).unwrap();
    let len = serialize(
        &KeyExchangeRequest {
            version: handshake::HANDSHAKE_VERSION,
            static_pub: initiator_kp.public(),
            ephemeral_pub: initiator.ephemeral_pub(),
        },
        &mut buf,
    )
    .unwrap();
    stream.write_all(&buf[..len]).await.unwrap();
    stream.read_exact(&mut buf[..Header::LEN]).await.unwrap();
    let resp_len = deserialize_encryption_header(&buf[..Header::ENCRYPTION_INFO_LEN]).unwrap();
    stream
        .read_exact(&mut buf[Header::LEN..resp_len])
        .await
        .unwrap();
    let header = deserialize_header(&buf[..Header::LEN]).unwrap();
    let resp: KeyExchangeResponse = deserialize_body(&header, &buf[Header::LEN..]).unwrap();
    let session = initiator
        .finish(&resp.ephemeral_pub, &resp.confirm)
        .unwrap();

    // Install it the way the production initiator does, and check the stream now reports an
    // established session against the responder's identity.
    stream.install_session(session, responder_kp.public());
    assert_eq!(
        stream.peer_static_pub(),
        Some(responder_kp.public()),
        "the stream must record the authenticated peer identity"
    );

    let plain = serialize(
        &Ack {
            ack_id: b"encrypted-after-handshake".to_vec(),
        },
        &mut buf,
    )
    .unwrap();
    let plaintext_body = buf[Header::LEN..plain].to_vec();
    stream.encrypt_outgoing(&mut buf[..plain]).unwrap();

    // The body must actually have changed on the wire.
    assert_ne!(
        &buf[Header::LEN..plain],
        plaintext_body.as_slice(),
        "encrypt_outgoing left the body unchanged"
    );

    stream.write_all(&buf[..plain]).await.unwrap();
    stream.read_exact(&mut buf[..Header::LEN]).await.unwrap();
    let len = deserialize_encryption_header(&buf[..Header::ENCRYPTION_INFO_LEN]).unwrap();
    stream.read_exact(&mut buf[Header::LEN..len]).await.unwrap();
    stream.decrypt_incoming(&mut buf[..len]).unwrap();
    let header = deserialize_header(&buf[..Header::LEN]).unwrap();
    let echoed: Ack = deserialize_body(&header, &buf[Header::LEN..]).unwrap();

    assert_eq!(echoed.ack_id, b"encrypted-after-handshake");
}
