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
use tokio::net::UdpSocket;

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

/// A listener and a pool pointed at it.
struct Loopback {
    pool: Pool,
    _control: RunStateControl,
}

impl Loopback {
    /// Both sides on the same protocol, with the keys registered where they need to be.
    async fn new(protocol: Protocol) -> Self {
        Self::build(protocol, protocol, true).await
    }

    /// `register_client_key` off leaves the responder without the initiators key, which is how a
    /// peer that was never provisioned looks.
    async fn build(server: Protocol, client: Protocol, register_client_key: bool) -> Self {
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

        let server_addr = incoming::listen_tcp(
            localhost,
            Echo,
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
        pool.replace_node_addrs(PEER, vec![server_addr]);

        Self { pool, _control }
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

/// An unprovisioned peer must be refused, and the initiator must learn why.
#[tokio::test]
async fn unregistered_key_is_rejected() {
    for protocol in [Protocol::Authenticated, Protocol::Encrypted] {
        let lb = Loopback::build(protocol, protocol, false).await;

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
        let lb = Loopback::build(server, client, true).await;

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
        let lb = Loopback::build(server, client, true).await;
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
