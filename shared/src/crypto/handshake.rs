//! Mutually authenticated key exchange for BeeMsg channels.
//!
//! Implements the Noise **KK** pattern over X25519: both peers hold a long-term ("static") keypair
//! and know the other's public key in advance, so a single round trip yields a mutually
//! authenticated, forward-secret session key. Management stores the public keys (see the `keys`
//! table) and every node downloads the list, which is what makes "K"nown/"K"nown the right pattern
//! - no key needs to be transmitted for the first time during the handshake.
//!
//! # Why this authenticates both sides
//! Four Diffie-Hellman operations are mixed into a running chaining key:
//!
//! | Term | Initiator computes | Purpose |
//! |------|--------------------|---------|
//! | `es` | `e_i · s_r` | binds the responder's static identity |
//! | `ss` | `s_i · s_r` | **mutual auth** - only the two holders of these static private keys can compute it |
//! | `ee` | `e_i · e_r` | forward secrecy for this session |
//! | `se` | `s_i · e_r` | binds the initiator's static identity |
//!
//! `ss` is the crux: an attacker without either static private key cannot produce it, so there is
//! no signature to verify - the DH itself is the proof of key possession. Both ephemeral public
//! keys are also mixed into the transcript hash, so no two sessions derive the same key.
//!
//! # Cross-tree compatibility
//! The C++ servers and the kernel client must reimplement this exactly. Choices made with that in
//! mind: **SHA-256** rather than WireGuard's BLAKE2s (SHA-256 is the only hash available to all
//! three of Rust, OpenSSL and the kernel crypto API without extra work), and a plain transcript
//! HMAC for handshake confirmation rather than Noise's empty-payload AEAD authenticator. See
//! `kdf_vector` in the tests for the fixed vectors all implementations must agree on.

use crate::types::{AuthSecret, StaticPubKey};
use anyhow::{Result, anyhow, bail};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, ReusableSecret, SharedSecret, StaticSecret};
use zeroize::{Zeroize, Zeroizing};

type HmacSha256 = Hmac<Sha256>;

/// Identifies this exact construction. Any change to the primitives, the mixing order or the
/// message layout must come with a new name, so mismatched peers fail at the handshake rather than
/// silently deriving different keys.
const PROTOCOL_NAME: &str = "BeeGFS_KK_25519_AESGCM_SHA256";

/// Wire format version carried in the first handshake message.
pub const HANDSHAKE_VERSION: u32 = 1;

/// An ephemeral public key as it appears on the wire.
pub type EphemeralPub = [u8; 32];

/// The nonce base for both directions of a freshly established session.
///
/// Zero is sound here: each session derives fresh random keys, so the per-direction message counter
/// alone already guarantees every (key, nonce) pair is unique.
const SESSION_NONCE_BASE: [u8; 12] = [0; 12];

// ---------------------------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------------------------

fn hmac(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC-SHA256 accepts any key length");
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

/// The two-output HKDF used by Noise (and WireGuard): one extract followed by chained expands.
///
/// Deliberately hand-rolled rather than taken from an HKDF crate - it is five lines, and this is
/// the form the kernel client will reimplement on `crypto_shash`.
fn hkdf2(chaining_key: &[u8; 32], input: &[u8]) -> ([u8; 32], [u8; 32]) {
    let mut temp = hmac(chaining_key, &[input]);
    let out1 = hmac(&temp, &[&[0x01]]);
    let out2 = hmac(&temp, &[&out1, &[0x02]]);
    temp.zeroize();
    (out1, out2)
}

/// [`hkdf2`] with a third output, used by [`Chain::split`] to get two data keys plus a separate
/// confirmation key. The confirmation key is domain-separated on purpose: reusing a data key to
/// also key the HMAC would be needless cross-primitive key reuse.
fn hkdf3(chaining_key: &[u8; 32], input: &[u8]) -> ([u8; 32], [u8; 32], [u8; 32]) {
    let mut temp = hmac(chaining_key, &[input]);
    let out1 = hmac(&temp, &[&[0x01]]);
    let out2 = hmac(&temp, &[&out1, &[0x02]]);
    let out3 = hmac(&temp, &[&out2, &[0x03]]);
    temp.zeroize();
    (out1, out2, out3)
}

/// Derives the shared datagram key from the authentication secret.
///
/// See [`super::Session::for_datagrams`] for why this exists and why it is weak.
pub(super) fn derive_datagram_key(secret: AuthSecret) -> [u8; 32] {
    hmac(b"BeeGFS datagram key v1", &[&secret.to_le_bytes()])
}

/// Rejects a degenerate DH result.
///
/// `was_contributory` is false when the peer sent a low-order point, which forces the shared secret
/// to a known constant and would let an attacker fix the session key.
fn dh(secret: &impl Dh, peer: &PublicKey) -> Result<[u8; 32]> {
    let shared = secret.agree(peer);

    if !shared.was_contributory() {
        bail!("Peer sent a low order public key");
    }

    Ok(shared.to_bytes())
}

/// Lets [`dh`] accept both long-term and ephemeral secrets.
trait Dh {
    fn agree(&self, peer: &PublicKey) -> SharedSecret;
}

impl Dh for StaticSecret {
    fn agree(&self, peer: &PublicKey) -> SharedSecret {
        self.diffie_hellman(peer)
    }
}

impl Dh for ReusableSecret {
    fn agree(&self, peer: &PublicKey) -> SharedSecret {
        self.diffie_hellman(peer)
    }
}

// ---------------------------------------------------------------------------------------------
// Chaining state
// ---------------------------------------------------------------------------------------------

/// The running handshake state: a chaining key that absorbs the DH outputs and a transcript hash
/// that absorbs everything sent on the wire.
struct Chain {
    /// Chaining key - secret, mixed with each DH result.
    ck: [u8; 32],
    /// Transcript hash - not secret, binds the handshake to exactly these messages.
    h: [u8; 32],
}

impl Drop for Chain {
    fn drop(&mut self) {
        self.ck.zeroize();
    }
}

impl Chain {
    /// `h = PROTOCOL_NAME` zero padded to 32 bytes (Noise hashes it only if longer), `ck = h`.
    fn new() -> Self {
        let name = PROTOCOL_NAME.as_bytes();
        let mut h = [0u8; 32];

        if name.len() <= 32 {
            h[..name.len()].copy_from_slice(name);
        } else {
            h = Sha256::digest(name).into();
        }

        Self { ck: h, h }
    }

    /// `h = SHA256(h || data)`
    fn mix_hash(&mut self, data: &[u8]) {
        let mut hasher = Sha256::new();
        hasher.update(self.h);
        hasher.update(data);
        self.h = hasher.finalize().into();
    }

    /// `ck = HKDF(ck, dh_output)` - absorbs one DH result.
    fn mix_key(&mut self, dh_output: &[u8; 32]) {
        let (ck, _) = hkdf2(&self.ck, dh_output);
        self.ck.zeroize();
        self.ck = ck;
    }

    /// Final key derivation: initiator-to-responder key, responder-to-initiator key, and the key
    /// used for the handshake confirmation HMAC.
    fn split(&self) -> ([u8; 32], [u8; 32], [u8; 32]) {
        hkdf3(&self.ck, &[])
    }
}

// ---------------------------------------------------------------------------------------------
// Long-term identity
// ---------------------------------------------------------------------------------------------

/// A nodes long-term X25519 identity keypair.
pub struct StaticKeypair {
    secret: StaticSecret,
    public: StaticPubKey,
}

impl std::fmt::Debug for StaticKeypair {
    /// Prints the public half only - the private key must never reach a log line.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "StaticKeypair({})", self.public)
    }
}

impl StaticKeypair {
    /// Generates a fresh keypair from the system RNG.
    pub fn generate() -> Self {
        Self::from_secret_bytes(StaticSecret::random().to_bytes())
    }

    pub fn from_secret_bytes(bytes: [u8; 32]) -> Self {
        let secret = StaticSecret::from(bytes);
        let public = StaticPubKey::from(PublicKey::from(&secret).to_bytes());

        Self { secret, public }
    }

    /// Loads a keypair from a file containing the raw 32 byte private key.
    pub fn from_key_file(path: &std::path::Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .map_err(|err| anyhow!("Could not read private key file {path:?}: {err}"))?;

        let bytes: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
            anyhow!(
                "Private key file {path:?} must contain exactly 32 raw bytes, got {}",
                bytes.len()
            )
        })?;

        Ok(Self::from_secret_bytes(bytes))
    }

    pub fn public(&self) -> StaticPubKey {
        self.public
    }

    /// The raw private key bytes, so a freshly generated key can be written to disk.
    ///
    /// Zeroized on drop. This is the secret half - never log it, never send it anywhere.
    pub fn secret_bytes(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(self.secret.to_bytes())
    }
}

// ---------------------------------------------------------------------------------------------
// Initiator
// ---------------------------------------------------------------------------------------------

/// Initiator side state, held between sending the request and receiving the response.
pub struct Initiator {
    chain: Chain,
    ephemeral: ReusableSecret,
    ephemeral_pub: EphemeralPub,
    /// Our own static secret, needed again for `se` once the response arrives.
    static_secret: StaticSecret,
}

impl Initiator {
    /// Starts a handshake against a peer whose static public key we already know.
    ///
    /// Mixes the pre-message keys and the first message: `mix_hash(s_i), mix_hash(s_r)`, then
    /// `mix_hash(e_i), mix_key(es), mix_key(ss)`.
    pub fn start(local: &StaticKeypair, peer_static: StaticPubKey) -> Result<Self> {
        let peer_static_pub = PublicKey::from(*peer_static.as_bytes());

        let ephemeral = ReusableSecret::random();
        let ephemeral_pub = PublicKey::from(&ephemeral).to_bytes();

        let mut chain = Chain::new();

        // Pre-messages: both static public keys are known to both sides up front.
        chain.mix_hash(local.public.as_bytes());
        chain.mix_hash(peer_static.as_bytes());

        // Message 1: -> e, es, ss
        chain.mix_hash(&ephemeral_pub);
        chain.mix_key(&dh(&ephemeral, &peer_static_pub)?);
        chain.mix_key(&dh(&local.secret, &peer_static_pub)?);

        Ok(Self {
            chain,
            ephemeral,
            ephemeral_pub,
            static_secret: local.secret.clone(),
        })
    }

    /// The ephemeral public key to put into the first handshake message.
    pub fn ephemeral_pub(&self) -> EphemeralPub {
        self.ephemeral_pub
    }

    /// Completes the handshake from the responder's reply.
    ///
    /// Mixes message 2 (`<- e, ee, se`), then verifies `confirm` before handing back a session -
    /// so a peer that does not hold the expected static private key fails here, rather than later
    /// on undecryptable traffic.
    pub fn finish(
        mut self,
        peer_ephemeral: &EphemeralPub,
        confirm: &[u8; 32],
    ) -> Result<super::Session> {
        let peer_ephemeral_pub = PublicKey::from(*peer_ephemeral);

        // Message 2: <- e, ee, se
        self.chain.mix_hash(peer_ephemeral);
        self.chain
            .mix_key(&dh(&self.ephemeral, &peer_ephemeral_pub)?);
        self.chain
            .mix_key(&dh(&self.static_secret, &peer_ephemeral_pub)?);

        let (mut k_i2r, mut k_r2i, mut k_confirm) = self.chain.split();

        let verified = verify_confirm(&k_confirm, &self.chain.h, confirm);
        k_confirm.zeroize();

        if let Err(err) = verified {
            k_i2r.zeroize();
            k_r2i.zeroize();
            return Err(err);
        }

        // We are the initiator: we send with k_i2r and receive with k_r2i.
        let session = super::Session::new(
            super::DirKey::new(&k_i2r, SESSION_NONCE_BASE),
            super::DirKey::new(&k_r2i, SESSION_NONCE_BASE),
        );

        k_i2r.zeroize();
        k_r2i.zeroize();

        Ok(session)
    }
}

// ---------------------------------------------------------------------------------------------
// Responder
// ---------------------------------------------------------------------------------------------

/// What the responder produces: the reply to send, and the session to install afterwards.
#[derive(Debug)]
pub struct Response {
    pub ephemeral_pub: EphemeralPub,
    pub confirm: [u8; 32],
    pub session: super::Session,
}

/// Completes the handshake from the responder's side in one step.
///
/// `peer_static` must be the key the caller looked up in its allow list - that lookup is the
/// authentication decision, so an unknown key must be rejected before calling this.
pub fn respond(
    local: &StaticKeypair,
    peer_static: StaticPubKey,
    peer_ephemeral: &EphemeralPub,
) -> Result<Response> {
    let peer_static_pub = PublicKey::from(*peer_static.as_bytes());
    let peer_ephemeral_pub = PublicKey::from(*peer_ephemeral);

    let mut chain = Chain::new();

    // Pre-messages, in the same order the initiator used: initiator's key first.
    chain.mix_hash(peer_static.as_bytes());
    chain.mix_hash(local.public.as_bytes());

    // Message 1: -> e, es, ss. Mirrored: our static secret against their ephemeral and static.
    chain.mix_hash(peer_ephemeral);
    chain.mix_key(&dh(&local.secret, &peer_ephemeral_pub)?);
    chain.mix_key(&dh(&local.secret, &peer_static_pub)?);

    // Message 2: <- e, ee, se
    let ephemeral = ReusableSecret::random();
    let ephemeral_pub = PublicKey::from(&ephemeral).to_bytes();

    chain.mix_hash(&ephemeral_pub);
    chain.mix_key(&dh(&ephemeral, &peer_ephemeral_pub)?);
    chain.mix_key(&dh(&ephemeral, &peer_static_pub)?);

    let (mut k_i2r, mut k_r2i, mut k_confirm) = chain.split();

    let confirm = hmac(&k_confirm, &[&chain.h]);
    k_confirm.zeroize();

    // We are the responder: we send with k_r2i and receive with k_i2r.
    let session = super::Session::new(
        super::DirKey::new(&k_r2i, SESSION_NONCE_BASE),
        super::DirKey::new(&k_i2r, SESSION_NONCE_BASE),
    );

    k_i2r.zeroize();
    k_r2i.zeroize();

    Ok(Response {
        ephemeral_pub,
        confirm,
        session,
    })
}

/// Constant time check of the handshake confirmation HMAC (`Mac::verify_slice` does not short
/// circuit).
fn verify_confirm(key: &[u8; 32], transcript: &[u8; 32], tag: &[u8; 32]) -> Result<()> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC-SHA256 accepts any key length");
    mac.update(transcript);

    mac.verify_slice(tag).map_err(|_| {
        anyhow!(
            "Handshake confirmation did not verify - the peer does not hold the private key \
             matching the public key we have for it, or the handshake was tampered with"
        )
    })
}

#[cfg(test)]
mod test {
    use super::*;

    fn keypair(seed: u8) -> StaticKeypair {
        StaticKeypair::from_secret_bytes([seed; 32])
    }

    /// A full handshake must leave both sides with matching, distinct directional keys.
    #[test]
    fn handshake_round_trip() {
        let initiator_kp = keypair(1);
        let responder_kp = keypair(2);

        let init = Initiator::start(&initiator_kp, responder_kp.public()).unwrap();
        let resp = respond(&responder_kp, initiator_kp.public(), &init.ephemeral_pub()).unwrap();
        let init_session = init.finish(&resp.ephemeral_pub, &resp.confirm).unwrap();

        // What the initiator encrypts, the responder must decrypt - and vice versa.
        assert_round_trip(&init_session, &resp.session);
        assert_round_trip(&resp.session, &init_session);
    }

    /// Two runs with the same static keys must never derive the same session key: the ephemeral
    /// keys are fresh each time and both are mixed into the transcript.
    #[test]
    fn sessions_are_unique_per_handshake() {
        let initiator_kp = keypair(1);
        let responder_kp = keypair(2);

        let first = {
            let init = Initiator::start(&initiator_kp, responder_kp.public()).unwrap();
            let resp =
                respond(&responder_kp, initiator_kp.public(), &init.ephemeral_pub()).unwrap();
            init.finish(&resp.ephemeral_pub, &resp.confirm).unwrap();
            resp.confirm
        };

        let second = {
            let init = Initiator::start(&initiator_kp, responder_kp.public()).unwrap();
            let resp =
                respond(&responder_kp, initiator_kp.public(), &init.ephemeral_pub()).unwrap();
            init.finish(&resp.ephemeral_pub, &resp.confirm).unwrap();
            resp.confirm
        };

        assert_ne!(
            first, second,
            "handshake transcript repeated across sessions"
        );
    }

    /// The `ss` term is what authenticates the peers. An initiator talking to a responder that
    /// holds a different static key must fail at the confirmation check, not later.
    #[test]
    fn wrong_responder_key_fails_confirmation() {
        let initiator_kp = keypair(1);
        let expected_responder = keypair(2);
        let actual_responder = keypair(3);

        // The initiator believes it is talking to `expected_responder`.
        let init = Initiator::start(&initiator_kp, expected_responder.public()).unwrap();
        // But an imposter answers.
        let resp = respond(
            &actual_responder,
            initiator_kp.public(),
            &init.ephemeral_pub(),
        )
        .unwrap();

        let err = init
            .finish(&resp.ephemeral_pub, &resp.confirm)
            .expect_err("imposter responder must be rejected");
        assert!(err.to_string().contains("Handshake confirmation"));
    }

    /// Likewise, a responder that has the wrong static key on file for the initiator derives a
    /// different chain, so its confirmation will not verify on the initiator side.
    #[test]
    fn wrong_initiator_key_fails_confirmation() {
        let initiator_kp = keypair(1);
        let responder_kp = keypair(2);
        let impersonated = keypair(4);

        let init = Initiator::start(&initiator_kp, responder_kp.public()).unwrap();
        // Responder was told the initiator is somebody else.
        let resp = respond(&responder_kp, impersonated.public(), &init.ephemeral_pub()).unwrap();

        init.finish(&resp.ephemeral_pub, &resp.confirm)
            .expect_err("mismatched initiator identity must be rejected");
    }

    #[test]
    fn tampered_confirmation_is_rejected() {
        let initiator_kp = keypair(1);
        let responder_kp = keypair(2);

        let init = Initiator::start(&initiator_kp, responder_kp.public()).unwrap();
        let resp = respond(&responder_kp, initiator_kp.public(), &init.ephemeral_pub()).unwrap();

        let mut confirm = resp.confirm;
        confirm[0] ^= 0x01;

        init.finish(&resp.ephemeral_pub, &confirm)
            .expect_err("tampered confirmation must be rejected");
    }

    /// A low order ephemeral public key forces the DH output to a constant, which would let an
    /// attacker pin the session key. It must be rejected outright.
    #[test]
    fn low_order_peer_key_is_rejected() {
        let initiator_kp = keypair(1);
        let responder_kp = keypair(2);

        let init = Initiator::start(&initiator_kp, responder_kp.public()).unwrap();

        let err = init
            .finish(&[0u8; 32], &[0u8; 32])
            .expect_err("all-zero ephemeral key must be rejected");
        assert!(err.to_string().contains("low order"));
    }

    /// A responder must reject a static key that is not a valid curve point in the same way.
    #[test]
    fn low_order_static_key_is_rejected_by_responder() {
        let responder_kp = keypair(2);
        let init_ephemeral = [7u8; 32];

        let err = respond(
            &responder_kp,
            StaticPubKey::from([0u8; 32]),
            &init_ephemeral,
        )
        .expect_err("all-zero static key must be rejected");
        assert!(err.to_string().contains("low order"));
    }

    /// Cross-tree vector. The C++ servers and the kernel client must reproduce exactly these
    /// values from the same fixed inputs; a mismatch means the implementations have diverged and
    /// will fail to interoperate. Inputs are fully deterministic - all four keys are fixed, so no
    /// randomness is involved.
    ///
    /// If this test fails after an intentional protocol change, bump `PROTOCOL_NAME` and
    /// regenerate the expectations here *and* in the other two trees.
    #[test]
    fn kdf_vector() {
        // Fixed private keys: initiator static/ephemeral = 0x01/0x03, responder = 0x02/0x04.
        // `StaticSecret` stands in for the ephemerals so they are deterministic: it is
        // byte-for-byte equivalent to `ReusableSecret` (both store raw bytes and clamp at use).
        let initiator_kp = keypair(1);
        let responder_kp = keypair(2);
        let init_ephemeral = StaticSecret::from([3u8; 32]);
        let resp_ephemeral = StaticSecret::from([4u8; 32]);

        let init_e_pub = PublicKey::from(&init_ephemeral).to_bytes();
        let resp_e_pub = PublicKey::from(&resp_ephemeral).to_bytes();
        let init_s_pub = PublicKey::from(*initiator_kp.public.as_bytes());
        let resp_s_pub = PublicKey::from(*responder_kp.public.as_bytes());

        // Drive the chain by hand so the ephemeral keys are fixed rather than random.
        let mut chain = Chain::new();
        chain.mix_hash(initiator_kp.public.as_bytes());
        chain.mix_hash(responder_kp.public.as_bytes());
        chain.mix_hash(&init_e_pub);
        chain.mix_key(&dh(&init_ephemeral, &resp_s_pub).unwrap());
        chain.mix_key(&dh(&initiator_kp.secret, &resp_s_pub).unwrap());
        chain.mix_hash(&resp_e_pub);
        chain.mix_key(&dh(&init_ephemeral, &PublicKey::from(resp_e_pub)).unwrap());
        chain.mix_key(&dh(&initiator_kp.secret, &PublicKey::from(resp_e_pub)).unwrap());

        let (k_i2r, k_r2i, k_confirm) = chain.split();

        // Sanity check that the responder derives the same thing from the mirrored operations.
        let mut mirror = Chain::new();
        mirror.mix_hash(initiator_kp.public.as_bytes());
        mirror.mix_hash(responder_kp.public.as_bytes());
        mirror.mix_hash(&init_e_pub);
        mirror.mix_key(&dh(&responder_kp.secret, &PublicKey::from(init_e_pub)).unwrap());
        mirror.mix_key(&dh(&responder_kp.secret, &init_s_pub).unwrap());
        mirror.mix_hash(&resp_e_pub);
        mirror.mix_key(&dh(&resp_ephemeral, &PublicKey::from(init_e_pub)).unwrap());
        mirror.mix_key(&dh(&resp_ephemeral, &init_s_pub).unwrap());
        assert_eq!(hex(&mirror.h), hex(&chain.h), "transcripts must match");
        assert_eq!(
            hex(&mirror.split().0),
            hex(&k_i2r),
            "both sides must derive the same keys"
        );

        assert_eq!(hex(&init_e_pub), INIT_EPHEMERAL_PUB, "e_i public diverged");
        assert_eq!(hex(&resp_e_pub), RESP_EPHEMERAL_PUB, "e_r public diverged");
        assert_eq!(hex(&chain.h), TRANSCRIPT_HASH, "transcript hash diverged");
        assert_eq!(hex(&k_i2r), KEY_I2R, "initiator->responder key diverged");
        assert_eq!(hex(&k_r2i), KEY_R2I, "responder->initiator key diverged");
        assert_eq!(hex(&k_confirm), KEY_CONFIRM, "confirmation key diverged");
    }

    // Expected values for `kdf_vector`, generated from this implementation with
    // PROTOCOL_NAME = "BeeGFS_KK_25519_AESGCM_SHA256" and the fixed private keys
    // s_i = 01*32, s_r = 02*32, e_i = 03*32, e_r = 04*32.
    //
    // The C++ servers and the kernel client must reproduce these exactly. The two ephemeral public
    // keys are plain X25519 base point multiplications and serve as a check that the curve
    // implementation agrees before any of the chaining logic is suspected.
    const INIT_EPHEMERAL_PUB: &str =
        "5dfedd3b6bd47f6fa28ee15d969d5bb0ea53774d488bdaf9df1c6e0124b3ef22";
    const RESP_EPHEMERAL_PUB: &str =
        "ac01b2209e86354fb853237b5de0f4fab13c7fcbf433a61c019369617fecf10b";
    const TRANSCRIPT_HASH: &str =
        "c3901a52afc7ddfaf767d06e4a71765f9697ade9eee30b98d7d70701498527c6";
    const KEY_I2R: &str = "f274965f5ac28818ab570470f9c4f70361b0a4c9a33280f009f1067caf7774ea";
    const KEY_R2I: &str = "fdc9ec8f84376790bf3489e34f07775a8e3fe16b5d06a607aac313f5839ebc64";
    const KEY_CONFIRM: &str = "9d5fec84550431f1a85cc47c001e15ab0a8aeb182e7d4fd770a2cbc56f787a95";

    fn assert_round_trip(sender: &super::super::Session, receiver: &super::super::Session) {
        const PLAIN: &[u8] = b"Hello BeeGFS!aaa";
        let mut buf = PLAIN.to_vec();
        buf.extend([0u8; super::super::AES_TAG_LEN]);

        sender.encrypt(0, &mut buf).unwrap();
        receiver.decrypt(0, &mut buf).unwrap();
        assert_eq!(&buf[..PLAIN.len()], PLAIN);
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
