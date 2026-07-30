//! Conformance tests for the BeeMsg key exchange, against an independent implementation.
//!
//! This file deliberately does **not** use `shared::crypto::handshake` to compute anything. It
//! reimplements the protocol from the specification below - its own chaining state, its own HKDF,
//! its own BeeMsg framing - and then makes that implementation talk to the real one over a real TCP
//! socket, in both directions. A test that called the implementation to check the implementation
//! would only prove self-consistency; this proves the protocol is what it is documented to be,
//! which is what the C++ servers and the kernel client need in order to interoperate.
//!
//! What is and is not independent here:
//!
//! * The chaining key, transcript hash, HKDF, key split, confirmation MAC, message layout, header
//!   framing, AES-GCM nonce derivation and AAD split are all written out again below. These are the
//!   parts that carry protocol risk, and a mistake in the real code shows up as a test failure.
//! * X25519 and the AES-GCM and SHA-256 primitives come from the same crates the implementation
//!   uses. Reimplementing a curve or a hash would test those crates, not this protocol. Curve
//!   agreement with a third implementation is instead pinned by [`INIT_EPHEMERAL_PUB`] and
//!   [`RESP_EPHEMERAL_PUB`], which were generated from an independent RFC 7748 Montgomery ladder.
//!
//! # The protocol
//!
//! Noise **KK** over X25519 with SHA-256. Both peers hold a long-term ("static") keypair and know
//! the other's public key in advance; management distributes the public keys via the `keys` table.
//!
//! ```text
//! h  = "BeeGFS_KK_25519_AESGCM_SHA256" zero padded to 32 bytes
//! ck = h
//!
//! mix_hash(d) : h  = SHA256(h || d)
//! mix_key(dh) : ck = HKDF(ck, dh)[0]
//!
//! pre-message : mix_hash(s_i_pub); mix_hash(s_r_pub)         # initiator's key first
//! message 1   : mix_hash(e_i_pub); mix_key(es); mix_key(ss)  # initiator -> responder
//! message 2   : mix_hash(e_r_pub); mix_key(ee); mix_key(se)  # responder -> initiator
//! split       : k_i2r, k_r2i, k_confirm = HKDF3(ck, "")
//!
//! es = e_i * s_r    ss = s_i * s_r    ee = e_i * e_r    se = s_i * e_r
//! ```
//!
//! `ss` is what authenticates both peers: only the holders of those two static private keys can
//! compute it, so no signature is required. The responder proves it got there by sending
//! `confirm = HMAC-SHA256(k_confirm, h)`, which the initiator verifies before trusting the session.
//!
//! HKDF is the Noise/WireGuard chained-expand form, not RFC 5869 verbatim:
//!
//! ```text
//! temp = HMAC(ck, ikm);  out1 = HMAC(temp, 0x01);  out2 = HMAC(temp, out1 || 0x02);  ...
//! ```
//!
//! Each direction gets its own key. Both handshake messages travel in the clear; everything after
//! them is AES-256-GCM with a 12 byte nonce that is all zero except the big endian message counter
//! in its last 8 bytes, the first 8 bytes of the frame (magic + length) as additional data, and the
//! tag in the final 16 bytes.

use aes_gcm::aead::AeadInPlace;
use aes_gcm::{Aes256Gcm, Key, KeyInit, Tag};
use anyhow::{Result, bail};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use shared::bee_msg::misc::{Ack, KeyExchangeRequest, KeyExchangeResponse};
use shared::bee_msg::{Header, Msg, deserialize, serialize};
use shared::conn::incoming::listen_tcp;
use shared::conn::key_store::KeyStore;
use shared::conn::msg_dispatch::{DispatchRequest, Request};
use shared::conn::outgoing::Pool;
use shared::crypto::handshake::StaticKeypair;
use shared::run_state;
use shared::types::{AuthSecret, StaticPubKey, Uid};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use x25519_dalek::{PublicKey, StaticSecret};

// ---------------------------------------------------------------------------------------------
// Pinned vectors
// ---------------------------------------------------------------------------------------------

/// Fixed private keys for the vectors. Must match `kdf_vector` in src/crypto/handshake.rs.
const VECTOR_S_I: [u8; 32] = [1; 32];
const VECTOR_S_R: [u8; 32] = [2; 32];
const VECTOR_E_I: [u8; 32] = [3; 32];
const VECTOR_E_R: [u8; 32] = [4; 32];

// Expected values. Generated independently of this crate's curve implementation (a pure RFC 7748
// Montgomery ladder), so the two ephemeral public keys also pin X25519 agreement itself.
const INIT_EPHEMERAL_PUB: &str = "5dfedd3b6bd47f6fa28ee15d969d5bb0ea53774d488bdaf9df1c6e0124b3ef22";
const RESP_EPHEMERAL_PUB: &str = "ac01b2209e86354fb853237b5de0f4fab13c7fcbf433a61c019369617fecf10b";
const TRANSCRIPT_HASH: &str = "c3901a52afc7ddfaf767d06e4a71765f9697ade9eee30b98d7d70701498527c6";
const KEY_I2R: &str = "f274965f5ac28818ab570470f9c4f70361b0a4c9a33280f009f1067caf7774ea";
const KEY_R2I: &str = "fdc9ec8f84376790bf3489e34f07775a8e3fe16b5d06a607aac313f5839ebc64";
const KEY_CONFIRM: &str = "9d5fec84550431f1a85cc47c001e15ab0a8aeb182e7d4fd770a2cbc56f787a95";

// ---------------------------------------------------------------------------------------------
// Independent implementation
// ---------------------------------------------------------------------------------------------

const PROTOCOL_NAME: &[u8] = b"BeeGFS_KK_25519_AESGCM_SHA256";
const HANDSHAKE_VERSION: u32 = 1;

/// Message ids, spelled out rather than taken from the crate, so an accidental renumbering shows up
/// as a failure instead of silently moving the wire protocol.
const ID_KEY_EXCHANGE_REQUEST: u16 = 4013;
const ID_KEY_EXCHANGE_RESPONSE: u16 = 4015;

const HEADER_LEN: usize = 36;
const TAG_LEN: usize = 16;
/// Bytes of the frame that stay in the clear and are authenticated as additional data.
const AAD_LEN: usize = 8;

type HmacSha256 = Hmac<Sha256>;

fn mac(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    // Disambiguated: `aes_gcm::KeyInit` is also in scope and also offers `new_from_slice`.
    let mut m = <HmacSha256 as Mac>::new_from_slice(key).unwrap();
    for p in parts {
        m.update(p);
    }
    m.finalize().into_bytes().into()
}

/// The Noise/WireGuard chained-expand HKDF, for `N` outputs.
fn hkdf<const N: usize>(chaining_key: &[u8; 32], input: &[u8]) -> [[u8; 32]; N] {
    let temp = mac(chaining_key, &[input]);
    let mut out = [[0u8; 32]; N];
    let mut prev: &[u8] = &[];
    let mut carry;

    for (i, slot) in out.iter_mut().enumerate() {
        carry = mac(&temp, &[prev, &[i as u8 + 1]]);
        *slot = carry;
        // Safe to leak the borrow forward: `carry` outlives this iteration via `slot`.
        prev = &slot[..];
    }

    out
}

fn public_of(secret: &[u8; 32]) -> [u8; 32] {
    PublicKey::from(&StaticSecret::from(*secret)).to_bytes()
}

/// Raw DH with the same contributory-behaviour rejection the implementation applies.
fn dh(secret: &[u8; 32], peer: &[u8; 32]) -> [u8; 32] {
    let shared = StaticSecret::from(*secret).diffie_hellman(&PublicKey::from(*peer));
    assert!(shared.was_contributory(), "low order peer key");
    shared.to_bytes()
}

/// The running handshake state, reimplemented from the module documentation.
struct Chain {
    ck: [u8; 32],
    h: [u8; 32],
}

impl Chain {
    fn new() -> Self {
        assert!(PROTOCOL_NAME.len() <= 32, "longer names would be hashed");
        let mut h = [0u8; 32];
        h[..PROTOCOL_NAME.len()].copy_from_slice(PROTOCOL_NAME);
        Self { ck: h, h }
    }

    fn mix_hash(&mut self, data: &[u8]) {
        let mut hasher = Sha256::new();
        hasher.update(self.h);
        hasher.update(data);
        self.h = hasher.finalize().into();
    }

    fn mix_key(&mut self, dh_output: &[u8; 32]) {
        self.ck = hkdf::<2>(&self.ck, dh_output)[0];
    }

    /// (k_i2r, k_r2i, k_confirm)
    fn split(&self) -> ([u8; 32], [u8; 32], [u8; 32]) {
        let [a, b, c] = hkdf::<3>(&self.ck, &[]);
        (a, b, c)
    }
}

/// Pre-message plus message 1, from the initiator's point of view.
fn initiator_msg1(s_i: &[u8; 32], s_r_pub: &[u8; 32], e_i: &[u8; 32]) -> (Chain, [u8; 32]) {
    let e_i_pub = public_of(e_i);

    let mut chain = Chain::new();
    chain.mix_hash(&public_of(s_i));
    chain.mix_hash(s_r_pub);
    chain.mix_hash(&e_i_pub);
    chain.mix_key(&dh(e_i, s_r_pub)); // es
    chain.mix_key(&dh(s_i, s_r_pub)); // ss

    (chain, e_i_pub)
}

/// Message 2 plus split, from the initiator's point of view.
fn initiator_msg2(
    chain: &mut Chain,
    s_i: &[u8; 32],
    e_i: &[u8; 32],
    e_r_pub: &[u8; 32],
) -> ([u8; 32], [u8; 32], [u8; 32]) {
    chain.mix_hash(e_r_pub);
    chain.mix_key(&dh(e_i, e_r_pub)); // ee
    chain.mix_key(&dh(s_i, e_r_pub)); // se
    chain.split()
}

/// The session keys a completed handshake yields: (k_i2r, k_r2i, k_confirm).
type SplitKeys = ([u8; 32], [u8; 32], [u8; 32]);

/// The whole responder side, mirrored.
fn responder(
    s_r: &[u8; 32],
    s_i_pub: &[u8; 32],
    e_i_pub: &[u8; 32],
    e_r: &[u8; 32],
) -> (Chain, [u8; 32], SplitKeys) {
    let e_r_pub = public_of(e_r);

    let mut chain = Chain::new();
    chain.mix_hash(s_i_pub);
    chain.mix_hash(&public_of(s_r));
    chain.mix_hash(e_i_pub);
    chain.mix_key(&dh(s_r, e_i_pub)); // es
    chain.mix_key(&dh(s_r, s_i_pub)); // ss
    chain.mix_hash(&e_r_pub);
    chain.mix_key(&dh(e_r, e_i_pub)); // ee
    chain.mix_key(&dh(e_r, s_i_pub)); // se

    let keys = chain.split();
    (chain, e_r_pub, keys)
}

// ---------------------------------------------------------------------------------------------
// Independent BeeMsg framing
// ---------------------------------------------------------------------------------------------

const MSG_PREFIX: u32 = 0x5346_4742;

/// Builds a complete BeeMsg frame by hand. `msg_len` covers the header, body and tag slot.
fn build_frame(msg_id: u16, body: &[u8]) -> Vec<u8> {
    let msg_len = (HEADER_LEN + body.len() + TAG_LEN) as u32;

    let mut f = Vec::with_capacity(msg_len as usize);
    f.extend(MSG_PREFIX.to_le_bytes()); // msg_prefix
    f.extend(msg_len.to_le_bytes()); // msg_len
    f.extend(0u16.to_le_bytes()); // msg_feature_flags
    f.push(0); // msg_compat_feature_flags
    f.push(0); // msg_flags
    f.extend(msg_id.to_le_bytes()); // msg_id
    f.extend(0u16.to_le_bytes()); // msg_target_id
    f.extend(0u32.to_le_bytes()); // msg_user_id
    f.extend(0u64.to_le_bytes()); // msg_seq
    f.extend(0u64.to_le_bytes()); // msg_seq_done
    assert_eq!(f.len(), HEADER_LEN, "hand built header must be 36 bytes");

    f.extend(body);
    f.extend([0u8; TAG_LEN]);
    assert_eq!(f.len(), msg_len as usize);

    f
}

/// (msg_len, msg_id)
fn parse_frame_header(buf: &[u8]) -> (usize, u16) {
    let prefix = u32::from_le_bytes(buf[0..4].try_into().unwrap());
    assert_eq!(prefix, MSG_PREFIX, "bad BeeMsg prefix");

    let msg_len = u32::from_le_bytes(buf[4..8].try_into().unwrap()) as usize;
    let msg_id = u16::from_le_bytes(buf[12..14].try_into().unwrap());

    (msg_len, msg_id)
}

/// The 12 byte AES-GCM nonce: all zero, with the counter big endian in the last 8 bytes.
fn nonce_for(counter: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&counter.to_be_bytes());
    n
}

/// Encrypts a serialized frame in place, per the layout in the module docs: first 8 bytes are
/// additional data and stay readable, the rest up to the tag is the ciphertext, tag goes last.
fn seal(key: &[u8; 32], counter: u64, frame: &mut [u8]) {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let clear_len = frame.len() - TAG_LEN;
    let (msg, tag_slot) = frame.split_at_mut(clear_len);
    let (aad, body) = msg.split_at_mut(AAD_LEN);

    let tag = cipher
        .encrypt_in_place_detached(&nonce_for(counter).into(), aad, body)
        .expect("encrypt");
    tag_slot.copy_from_slice(&tag);
}

/// Inverse of [`seal`].
fn open(key: &[u8; 32], counter: u64, frame: &mut [u8]) -> Result<()> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let clear_len = frame.len() - TAG_LEN;
    let (msg, tag_slot) = frame.split_at_mut(clear_len);
    let tag = Tag::clone_from_slice(tag_slot);
    let (aad, body) = msg.split_at_mut(AAD_LEN);

    cipher
        .decrypt_in_place_detached(&nonce_for(counter).into(), aad, body, &tag)
        .map_err(|_| anyhow::anyhow!("decrypt/authenticate failed"))
}

async fn read_frame(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; HEADER_LEN];
    stream.read_exact(&mut buf).await?;
    let (msg_len, _) = parse_frame_header(&buf);
    buf.resize(msg_len, 0);
    stream.read_exact(&mut buf[HEADER_LEN..]).await?;
    Ok(buf)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------------------------
// Vector conformance
// ---------------------------------------------------------------------------------------------

/// The independent implementation must reproduce the pinned vectors, and both of its own halves
/// must agree with each other.
#[test]
fn independent_implementation_matches_pinned_vectors() {
    let s_r_pub = public_of(&VECTOR_S_R);
    let (mut chain, e_i_pub) = initiator_msg1(&VECTOR_S_I, &s_r_pub, &VECTOR_E_I);
    let e_r_pub = public_of(&VECTOR_E_R);
    let (k_i2r, k_r2i, k_confirm) = initiator_msg2(&mut chain, &VECTOR_S_I, &VECTOR_E_I, &e_r_pub);

    // The mirrored responder must land on the same transcript and the same keys.
    let (r_chain, r_e_r_pub, r_keys) =
        responder(&VECTOR_S_R, &public_of(&VECTOR_S_I), &e_i_pub, &VECTOR_E_R);
    assert_eq!(hex(&r_e_r_pub), hex(&e_r_pub));
    assert_eq!(hex(&r_chain.h), hex(&chain.h), "transcripts diverged");
    assert_eq!(
        (hex(&r_keys.0), hex(&r_keys.1), hex(&r_keys.2)),
        (hex(&k_i2r), hex(&k_r2i), hex(&k_confirm)),
        "the two sides derived different keys"
    );

    assert_eq!(hex(&e_i_pub), INIT_EPHEMERAL_PUB, "e_i public diverged");
    assert_eq!(hex(&e_r_pub), RESP_EPHEMERAL_PUB, "e_r public diverged");
    assert_eq!(hex(&chain.h), TRANSCRIPT_HASH, "transcript hash diverged");
    assert_eq!(hex(&k_i2r), KEY_I2R, "initiator->responder key diverged");
    assert_eq!(hex(&k_r2i), KEY_R2I, "responder->initiator key diverged");
    assert_eq!(hex(&k_confirm), KEY_CONFIRM, "confirmation key diverged");
}

/// The message ids and frame geometry are wire protocol. Changing them breaks every other
/// implementation, so they are asserted against literals rather than read from the crate.
#[test]
fn wire_constants_are_stable() {
    assert_eq!(KeyExchangeRequest::ID, ID_KEY_EXCHANGE_REQUEST);
    assert_eq!(KeyExchangeResponse::ID, ID_KEY_EXCHANGE_RESPONSE);
    assert_eq!(Header::LEN, HEADER_LEN);
    assert_eq!(Header::ENCRYPTION_INFO_LEN, AAD_LEN);
}

// ---------------------------------------------------------------------------------------------
// Interop: foreign initiator -> real responder
// ---------------------------------------------------------------------------------------------

/// A responder built on the real implementation, standing in for management's handler.
#[derive(Clone, Debug)]
struct RealResponder {
    keypair: Arc<StaticKeypair>,
    key_store: Arc<KeyStore>,
}

impl DispatchRequest for RealResponder {
    async fn dispatch_request(&self, req: impl Request) -> Result<()> {
        let mut req = req;

        match req.header().msg_id() {
            KeyExchangeRequest::ID => {
                let msg: KeyExchangeRequest = req.deserialize_msg()?;

                if self.key_store.node_by_key(&msg.static_pub).is_none() {
                    bail!("unknown peer key {}", msg.static_pub);
                }

                let resp = shared::crypto::handshake::respond(
                    &self.keypair,
                    msg.static_pub,
                    &msg.ephemeral_pub,
                )?;

                req.install_session(resp.session, msg.static_pub);
                req.respond(&KeyExchangeResponse {
                    ephemeral_pub: resp.ephemeral_pub,
                    confirm: resp.confirm,
                })
                .await
            }

            Ack::ID => {
                let msg: Ack = req.deserialize_msg()?;
                req.respond(&Ack { ack_id: msg.ack_id }).await
            }

            id => bail!("unexpected msg id {id}"),
        }
    }
}

async fn spawn_real_responder(
    keypair: Arc<StaticKeypair>,
    key_store: Arc<KeyStore>,
) -> (SocketAddr, run_state::RunStateControl) {
    let (run_state, control) = run_state::new();

    let addr = listen_tcp(
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
        RealResponder { keypair, key_store },
        true,
        run_state,
    )
    .await
    .unwrap();

    (addr, control)
}

/// A hand-built handshake, from a peer that shares no protocol code with the implementation, must
/// be accepted - and the session keys both sides derive must match, proven by exchanging a real
/// encrypted message afterwards.
#[tokio::test]
async fn foreign_initiator_interoperates_with_real_responder() {
    let client_static = [0x11u8; 32];
    let client_ephemeral = [0x22u8; 32];

    let server_kp = Arc::new(StaticKeypair::generate());
    let server_pub: [u8; 32] = *server_kp.public().as_bytes();

    let key_store = Arc::new(KeyStore::new());
    key_store.insert(1, StaticPubKey::from(public_of(&client_static)));

    let (addr, _control) = spawn_real_responder(server_kp, key_store).await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // --- message 1, framed by hand ---
    let (mut chain, e_i_pub) = initiator_msg1(&client_static, &server_pub, &client_ephemeral);

    let mut body = Vec::new();
    body.extend(HANDSHAKE_VERSION.to_le_bytes());
    body.extend(public_of(&client_static));
    body.extend(e_i_pub);
    stream
        .write_all(&build_frame(ID_KEY_EXCHANGE_REQUEST, &body))
        .await
        .unwrap();

    // --- message 2 ---
    let frame = read_frame(&mut stream).await.unwrap();
    let (_, msg_id) = parse_frame_header(&frame);
    assert_eq!(msg_id, ID_KEY_EXCHANGE_RESPONSE, "unexpected reply");

    let e_r_pub: [u8; 32] = frame[HEADER_LEN..HEADER_LEN + 32].try_into().unwrap();
    let confirm: &[u8] = &frame[HEADER_LEN + 32..HEADER_LEN + 64];
    assert_ne!(e_r_pub, [0u8; 32], "responder refused the key exchange");

    let (k_i2r, k_r2i, k_confirm) =
        initiator_msg2(&mut chain, &client_static, &client_ephemeral, &e_r_pub);

    // Verifying this proves the real responder derived the same chaining key from the same four DH
    // operations in the same order.
    assert_eq!(
        hex(&mac(&k_confirm, &[&chain.h])),
        hex(confirm),
        "confirmation MAC mismatch: the two implementations derived different keys"
    );

    // --- an encrypted message, sealed with our own AES-GCM wiring ---
    // The Ack body itself is built with the crate's serializer: BeeSerde is not what is under test
    // here, the nonce/AAD/tag layout is.
    let mut buf = vec![0u8; 4096];
    let len = serialize(
        &Ack {
            ack_id: b"foreign-initiator".to_vec(),
        },
        &mut buf,
    )
    .unwrap();
    let mut frame = buf[..len].to_vec();

    let plaintext_body = frame[AAD_LEN..len - TAG_LEN].to_vec();
    seal(&k_i2r, 0, &mut frame);
    assert_ne!(
        frame[AAD_LEN..len - TAG_LEN],
        plaintext_body[..],
        "sealing left the body in the clear"
    );

    stream.write_all(&frame).await.unwrap();

    let mut reply = read_frame(&mut stream).await.unwrap();
    open(&k_r2i, 0, &mut reply).expect("the responder must reply under k_r2i with counter 0");
    let echoed: Ack = deserialize(&reply).unwrap();
    assert_eq!(echoed.ack_id, b"foreign-initiator");
}

/// The counter must advance per message and per direction, so the second exchange uses nonce 1.
/// The first message alone would pass even with a broken counter, since both sides start at zero.
#[tokio::test]
async fn foreign_initiator_counters_advance() {
    let client_static = [0x33u8; 32];
    let client_ephemeral = [0x44u8; 32];

    let server_kp = Arc::new(StaticKeypair::generate());
    let server_pub: [u8; 32] = *server_kp.public().as_bytes();

    let key_store = Arc::new(KeyStore::new());
    key_store.insert(1, StaticPubKey::from(public_of(&client_static)));

    let (addr, _control) = spawn_real_responder(server_kp, key_store).await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    let (mut chain, e_i_pub) = initiator_msg1(&client_static, &server_pub, &client_ephemeral);
    let mut body = Vec::new();
    body.extend(HANDSHAKE_VERSION.to_le_bytes());
    body.extend(public_of(&client_static));
    body.extend(e_i_pub);
    stream
        .write_all(&build_frame(ID_KEY_EXCHANGE_REQUEST, &body))
        .await
        .unwrap();

    let frame = read_frame(&mut stream).await.unwrap();
    let e_r_pub: [u8; 32] = frame[HEADER_LEN..HEADER_LEN + 32].try_into().unwrap();
    let (k_i2r, k_r2i, _) = initiator_msg2(&mut chain, &client_static, &client_ephemeral, &e_r_pub);

    let mut buf = vec![0u8; 4096];
    for counter in 0..3u64 {
        let payload = format!("message-{counter}").into_bytes();
        let len = serialize(
            &Ack {
                ack_id: payload.clone(),
            },
            &mut buf,
        )
        .unwrap();

        let mut frame = buf[..len].to_vec();
        seal(&k_i2r, counter, &mut frame);
        stream.write_all(&frame).await.unwrap();

        let mut reply = read_frame(&mut stream).await.unwrap();
        open(&k_r2i, counter, &mut reply)
            .unwrap_or_else(|err| panic!("reply {counter} did not open: {err:#}"));
        let echoed: Ack = deserialize(&reply).unwrap();
        assert_eq!(echoed.ack_id, payload);
    }
}

/// An initiator whose public key is not registered must not get a session. The real dispatcher
/// cannot substitute a `GenericResponse` for a typed handler, so refusal arrives as an all-zero
/// response.
#[tokio::test]
async fn foreign_initiator_with_unknown_key_is_refused() {
    let client_static = [0x55u8; 32];
    let client_ephemeral = [0x66u8; 32];

    let server_kp = Arc::new(StaticKeypair::generate());
    let server_pub: [u8; 32] = *server_kp.public().as_bytes();

    // Deliberately empty: the responder knows nobody.
    let key_store = Arc::new(KeyStore::new());

    let (addr, _control) = spawn_real_responder(server_kp, key_store).await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    let (_chain, e_i_pub) = initiator_msg1(&client_static, &server_pub, &client_ephemeral);
    let mut body = Vec::new();
    body.extend(HANDSHAKE_VERSION.to_le_bytes());
    body.extend(public_of(&client_static));
    body.extend(e_i_pub);
    stream
        .write_all(&build_frame(ID_KEY_EXCHANGE_REQUEST, &body))
        .await
        .unwrap();

    // The peer may refuse in band with an all-zero response, or by dropping the connection. Either
    // is acceptable; handing out a usable ephemeral key is not.
    if let Ok(frame) = read_frame(&mut stream).await {
        let e_r_pub = &frame[HEADER_LEN..HEADER_LEN + 32];
        assert_eq!(
            e_r_pub, [0u8; 32],
            "an unregistered key must not yield a real key exchange response"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Interop: real initiator -> foreign responder
// ---------------------------------------------------------------------------------------------

/// The other direction: the real [`Pool`] must be able to talk to a responder that shares no
/// protocol code with it. This covers the initiator half of the implementation, which the previous
/// tests exercise only as a peer.
#[tokio::test]
async fn real_initiator_interoperates_with_foreign_responder() {
    const PEER_UID: Uid = 4242;

    let server_static = [0x77u8; 32];
    let server_ephemeral = [0x88u8; 32];

    let client_kp = Arc::new(StaticKeypair::generate());
    let client_pub: [u8; 32] = *client_kp.public().as_bytes();

    let listener = TcpListener::bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    // The foreign responder, driven entirely by the independent implementation above.
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();

        // --- message 1 ---
        let frame = read_frame(&mut stream).await.unwrap();
        let (_, msg_id) = parse_frame_header(&frame);
        assert_eq!(msg_id, ID_KEY_EXCHANGE_REQUEST);

        let version = u32::from_le_bytes(frame[HEADER_LEN..HEADER_LEN + 4].try_into().unwrap());
        assert_eq!(version, HANDSHAKE_VERSION, "unexpected handshake version");

        let s_i_pub: [u8; 32] = frame[HEADER_LEN + 4..HEADER_LEN + 36].try_into().unwrap();
        let e_i_pub: [u8; 32] = frame[HEADER_LEN + 36..HEADER_LEN + 68].try_into().unwrap();
        assert_eq!(s_i_pub, client_pub, "initiator sent the wrong static key");

        // --- message 2 ---
        let (chain, e_r_pub, (k_i2r, k_r2i, k_confirm)) =
            responder(&server_static, &s_i_pub, &e_i_pub, &server_ephemeral);

        let mut body = Vec::new();
        body.extend(e_r_pub);
        body.extend(mac(&k_confirm, &[&chain.h]));
        stream
            .write_all(&build_frame(ID_KEY_EXCHANGE_RESPONSE, &body))
            .await
            .unwrap();

        // --- the real initiator's first encrypted request ---
        let mut request = read_frame(&mut stream).await.unwrap();
        open(&k_i2r, 0, &mut request).expect("the initiator must seal under k_i2r with counter 0");
        let received: Ack = deserialize(&request).unwrap();

        let mut buf = vec![0u8; 4096];
        let len = serialize(
            &Ack {
                ack_id: received.ack_id.clone(),
            },
            &mut buf,
        )
        .unwrap();
        let mut reply = buf[..len].to_vec();
        seal(&k_r2i, 0, &mut reply);
        stream.write_all(&reply).await.unwrap();

        received.ack_id
    });

    // The real initiator side.
    let key_store = Arc::new(KeyStore::new());
    key_store.insert(PEER_UID, StaticPubKey::from(public_of(&server_static)));

    let udp = Arc::new(
        UdpSocket::bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)))
            .await
            .unwrap(),
    );
    let pool = Pool::new(
        udp,
        4,
        Some(AuthSecret::hash_from_bytes("secret")),
        false,
        Some(client_kp),
        key_store,
    );
    pool.replace_node_addrs(PEER_UID, vec![addr]);

    let resp: Ack = pool
        .request(
            PEER_UID,
            &Ack {
                ack_id: b"real-initiator".to_vec(),
            },
        )
        .await
        .expect("the real initiator must interoperate with a foreign responder");

    assert_eq!(resp.ack_id, b"real-initiator");
    assert_eq!(server.await.unwrap(), b"real-initiator");
}

/// A foreign responder that fakes the confirmation MAC must be rejected: that MAC is the
/// initiator's only proof the peer holds the expected static private key.
#[tokio::test]
async fn real_initiator_rejects_bad_confirmation() {
    const PEER_UID: Uid = 4243;

    let server_static = [0x99u8; 32];
    let server_ephemeral = [0xaau8; 32];

    let client_kp = Arc::new(StaticKeypair::generate());

    let listener = TcpListener::bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let frame = read_frame(&mut stream).await.unwrap();

        let s_i_pub: [u8; 32] = frame[HEADER_LEN + 4..HEADER_LEN + 36].try_into().unwrap();
        let e_i_pub: [u8; 32] = frame[HEADER_LEN + 36..HEADER_LEN + 68].try_into().unwrap();

        let (_chain, e_r_pub, _) = responder(&server_static, &s_i_pub, &e_i_pub, &server_ephemeral);

        // Valid ephemeral key, garbage confirmation.
        let mut body = Vec::new();
        body.extend(e_r_pub);
        body.extend([0xffu8; 32]);
        let _ = stream
            .write_all(&build_frame(ID_KEY_EXCHANGE_RESPONSE, &body))
            .await;
    });

    let key_store = Arc::new(KeyStore::new());
    key_store.insert(PEER_UID, StaticPubKey::from(public_of(&server_static)));

    let udp = Arc::new(
        UdpSocket::bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)))
            .await
            .unwrap(),
    );
    let pool = Pool::new(
        udp,
        4,
        Some(AuthSecret::hash_from_bytes("secret")),
        false,
        Some(client_kp),
        key_store,
    );
    pool.replace_node_addrs(PEER_UID, vec![addr]);

    let res: Result<Ack> = pool
        .request(
            PEER_UID,
            &Ack {
                ack_id: b"nope".to_vec(),
            },
        )
        .await;

    let err = format!(
        "{:#}",
        res.expect_err("a forged confirmation must be rejected")
    );
    assert!(
        err.contains("confirmation"),
        "error should name the confirmation check, got: {err}"
    );
}
