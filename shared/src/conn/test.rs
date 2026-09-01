//! Loopback tests covering the framing of both protocols end to end.

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

/// A listener and a pool pointed at it, both configured from `server` / `client`.
struct Loopback {
    pool: Pool,
    _control: RunStateControl,
}

impl Loopback {
    async fn new(server: Protocol, client: Protocol) -> Self {
        let (run_state, _control) = run_state::new();

        let localhost = SocketAddr::from(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));

        let server_addr = incoming::listen_tcp(
            localhost,
            Echo,
            Arc::new(ConnConfig {
                protocol: server,
                ..Default::default()
            }),
            run_state.clone(),
        )
        .await
        .unwrap();

        let pool = Pool::new(
            Arc::new(UdpSocket::bind(localhost).await.unwrap()),
            2,
            Arc::new(ConnConfig {
                protocol: client,
                ..Default::default()
            }),
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

/// Sizes around the Noise record boundaries are included already so the encrypted protocol reuses
/// this list unchanged.
fn payload_lens() -> Vec<usize> {
    use crate::protocol::RECORD_PLAINTEXT_LEN as P;

    vec![0, 1, 1000, P - 1, P, P + 1, 2 * P, 2 * P + 1]
}

#[tokio::test]
async fn legacy_round_trip() {
    let lb = Loopback::new(Protocol::Legacy, Protocol::Legacy).await;

    for len in payload_lens() {
        let msg = TestMsg::of_len(len);
        assert_eq!(
            lb.request(&msg).await.unwrap(),
            msg,
            "payload of {len} bytes"
        );
    }
}

#[tokio::test]
async fn plain_round_trip() {
    let lb = Loopback::new(Protocol::Plain, Protocol::Plain).await;

    for len in payload_lens() {
        let msg = TestMsg::of_len(len);
        assert_eq!(
            lb.request(&msg).await.unwrap(),
            msg,
            "payload of {len} bytes"
        );
    }
}

/// The stream is reused across requests, so framing must stay aligned message after message.
#[tokio::test]
async fn plain_reuses_the_stream() {
    let lb = Loopback::new(Protocol::Plain, Protocol::Plain).await;

    for len in [1, 1000, 1, 70000, 5] {
        let msg = TestMsg::of_len(len);
        assert_eq!(
            lb.request(&msg).await.unwrap(),
            msg,
            "payload of {len} bytes"
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
        let lb = Loopback::new(server, client).await;
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
    };

    with_secret(Protocol::Legacy).check().unwrap();
    with_secret(Protocol::Plain).check().unwrap_err();
    with_secret(Protocol::Authenticated).check().unwrap_err();
    with_secret(Protocol::Encrypted).check().unwrap_err();
}
