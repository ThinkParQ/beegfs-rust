//! Identity authentication for gRPC.
//!
//! A client proves its identity once per channel by running the same Noise KK key exchange as
//! BeeMsg over the `Authenticate` RPC. It receives a token and sends that with every later request.
//!
//! The token is a bearer credential, so it is only as private as the transport carrying it. TLS is
//! what keeps it private. This is the same trade the BeeMsg `authenticated` mode makes: prove the
//! identity at setup, then trust the channel.

use super::*;
use crate::db::lookup::DbLookup;
use anyhow::{anyhow, ensure};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use shared::conn::handshake::{INIT_LEN, RESP_LEN, respond_to_payload};
use shared::conn::noise::Transport;
use shared::conn::protocol::{Protocol, StaticPubKey};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// gRPC metadata key carrying the token. Must be lowercase.
pub(crate) const TOKEN_HEADER: &str = "auth-token";

/// How long a token stays valid. A revoked key is rejected before this anyway, because the
/// interceptor re-reads the database on every request. This bounds how long a *stolen* token is
/// worth something.
const TOKEN_LIFETIME_SECS: u64 = 10 * 60;

const KEY_LEN: usize = StaticPubKey::LEN;
const EXPIRY_LEN: usize = size_of::<u64>();
/// HMAC-SHA256 output.
const MAC_LEN: usize = 32;
/// Everything the MAC is computed over.
const SIGNED_LEN: usize = KEY_LEN + EXPIRY_LEN;
const TOKEN_LEN: usize = SIGNED_LEN + MAC_LEN;

/// Key this process signs tokens with. Generated at startup, so every token dies with the process.
pub(crate) fn new_token_key() -> Result<hmac::Key> {
    let mut secret = [0u8; 32];
    SystemRandom::new()
        .fill(&mut secret)
        .map_err(|_| anyhow!("Generating the gRPC token key failed"))?;

    Ok(hmac::Key::new(hmac::HMAC_SHA256, &secret))
}

/// Mints a token for an identity that just proved itself.
fn issue_token(key: &hmac::Key, static_pub: StaticPubKey) -> String {
    let expiry = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        + TOKEN_LIFETIME_SECS;

    let mut token = [0u8; TOKEN_LEN];
    token[..KEY_LEN].copy_from_slice(static_pub.as_bytes());
    token[KEY_LEN..SIGNED_LEN].copy_from_slice(&expiry.to_be_bytes());
    let mac = hmac::sign(key, &token[..SIGNED_LEN]);
    token[SIGNED_LEN..].copy_from_slice(mac.as_ref());

    token.iter().map(|b| format!("{b:02x}")).collect()
}

/// Checks a token and returns the public key it was issued to.
///
/// Only proves this process minted the token and that it has not expired. The caller must still
/// resolve the key against the database, which is where revocation takes effect.
pub(crate) fn verify_token(key: &hmac::Key, token: &str) -> Result<StaticPubKey> {
    ensure!(
        token.len() == TOKEN_LEN * 2,
        "A token is {} characters, got {}",
        TOKEN_LEN * 2,
        token.len()
    );

    let mut raw = [0u8; TOKEN_LEN];
    for (i, byte) in raw.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&token[i * 2..i * 2 + 2], 16).context("Token is not hex")?;
    }

    hmac::verify(key, &raw[..SIGNED_LEN], &raw[SIGNED_LEN..])
        .map_err(|_| anyhow!("Token signature does not match"))?;

    let expiry = u64::from_be_bytes(raw[KEY_LEN..SIGNED_LEN].try_into()?);
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    ensure!(now < expiry, "Token expired");

    StaticPubKey::try_from(&raw[..KEY_LEN])
}

/// Serves the `Authenticate` RPC.
///
/// Registered without the interceptor: it cannot require the token it hands out.
#[derive(Debug)]
pub(crate) struct AuthenticationService {
    pub app: RuntimeApp,
    pub token_key: Arc<hmac::Key>,
}

#[tonic::async_trait]
impl pm::authentication_server::Authentication for AuthenticationService {
    async fn authenticate(
        &self,
        request: Request<pm::AuthenticateRequest>,
    ) -> std::result::Result<Response<pm::AuthenticateResponse>, Status> {
        let Protocol::Protected(ref protocol) = self.app.info.protocol else {
            return Err(Status::unimplemented(
                "This management node is not configured for identity based authentication",
            ));
        };

        let init = request.into_inner().handshake;
        if init.len() != INIT_LEN {
            return Err(Status::invalid_argument(format!(
                "A handshake is {INIT_LEN} bytes, got {}",
                init.len()
            )));
        }

        let lookup = DbLookup {
            db: self.app.db.clone(),
        };

        let mut handshake = vec![0u8; RESP_LEN];
        let (peer, mut transport) =
            match respond_to_payload(&init, &mut handshake, protocol, &lookup).await {
                Ok(res) => res,
                // The peer learns only that it was refused. Which check failed stays in our log,
                // because the caller is unauthenticated at this point.
                Err(err) => {
                    log::debug!("Rejected a gRPC key exchange: {err:#}");
                    return Err(Status::unauthenticated("Key exchange refused"));
                }
            };

        // Sealed, so a replayed request produces a token its sender cannot read.
        let token = issue_token(&self.token_key, peer.static_pub);
        let sealed = transport
            .seal_record(token.as_bytes())
            .map_err(|err| Status::internal(format!("Sealing the token failed: {err:#}")))?;
        handshake.extend_from_slice(&sealed[Transport::SEND_PREFIX_LEN..]);

        log::debug!(
            "gRPC client authenticated as identity {:?} ({})",
            peer.identity.name,
            peer.static_pub
        );

        Ok(Response::new(pm::AuthenticateResponse { handshake }))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use shared::conn::Lookup;
    use shared::conn::handshake::{finish_payload, init_payload};
    use shared::conn::protocol::{ProtectedProtocol, StaticKeypair, TransportProtectionMode};
    use shared::types::Uid;
    use std::net::SocketAddr;

    /// Accepts exactly one registered key, which is all the key exchange consults.
    #[derive(Clone, Debug)]
    struct OneKey(StaticPubKey);

    impl Lookup for OneKey {
        async fn identity_by_key(&self, key: StaticPubKey) -> Result<Option<PeerIdentity>> {
            Ok((key == self.0).then(|| PeerIdentity {
                name: Some("test".into()),
                node_uid: None,
            }))
        }

        async fn key_by_node(&self, _node: Uid) -> Result<Option<StaticPubKey>> {
            Ok(None)
        }

        async fn node_addrs(&self, _node: Uid) -> Result<Option<Vec<SocketAddr>>> {
            Ok(None)
        }
    }

    fn protocol() -> ProtectedProtocol {
        ProtectedProtocol::new(
            StaticKeypair::generate().unwrap(),
            TransportProtectionMode::Plain,
        )
    }

    /// Mirrors what the `Authenticate` handler does, so the tests below exercise the same path.
    async fn serve(
        init: &[u8],
        token_key: &hmac::Key,
        server: &ProtectedProtocol,
        lookup: &OneKey,
    ) -> Result<Vec<u8>> {
        let mut resp = vec![0u8; RESP_LEN];
        let (peer, mut transport) = respond_to_payload(init, &mut resp, server, lookup).await?;

        let token = issue_token(token_key, peer.static_pub);
        let sealed = transport.seal_record(token.as_bytes())?;
        resp.extend_from_slice(&sealed[Transport::SEND_PREFIX_LEN..]);

        Ok(resp)
    }

    #[tokio::test]
    async fn round_trip_yields_a_usable_token() {
        let server = protocol();
        let client = protocol();
        let lookup = OneKey(client.key_pair.public());
        let token_key = new_token_key().unwrap();

        let mut init = vec![0u8; INIT_LEN];
        let initiator = init_payload(&mut init, &client, server.key_pair.public()).unwrap();

        let resp = serve(&init, &token_key, &server, &lookup).await.unwrap();

        let mut transport = finish_payload(&resp, initiator, &client).unwrap();
        let sealed = &resp[RESP_LEN..];
        transport.record_in(sealed.len()).unwrap()[..].copy_from_slice(sealed);
        let mut token = vec![0u8; sealed.len()];
        let len = transport.open_record(sealed.len(), &mut token).unwrap();

        let token = std::str::from_utf8(&token[..len]).unwrap();
        assert_eq!(
            verify_token(&token_key, token).unwrap(),
            client.key_pair.public()
        );
    }

    /// Noise message 1 carries no freshness, so a captured request does complete the handshake
    /// again. The token must be useless to whoever replayed it.
    #[tokio::test]
    async fn a_replayed_request_yields_an_unreadable_token() {
        let server = protocol();
        let client = protocol();
        let lookup = OneKey(client.key_pair.public());
        let token_key = new_token_key().unwrap();

        let mut init = vec![0u8; INIT_LEN];
        let initiator = init_payload(&mut init, &client, server.key_pair.public()).unwrap();

        // The server does answer the replay. It cannot tell one from the original.
        let replayed = serve(&init, &token_key, &server, &lookup).await.unwrap();

        // The replayer holds the captured bytes but not the ephemeral private key behind them, so
        // it cannot derive the session the reply was sealed with. It fails on message 2 already,
        // before ever reaching the token.
        let mut attacker = vec![0u8; INIT_LEN];
        let attacker_init = init_payload(&mut attacker, &client, server.key_pair.public()).unwrap();
        finish_payload(&replayed, attacker_init, &client).unwrap_err();

        // The party that sent the original can, so replays cost the honest client nothing.
        finish_payload(&replayed, initiator, &client).unwrap();
    }

    #[tokio::test]
    async fn an_unregistered_key_is_refused() {
        let server = protocol();
        let client = protocol();
        let lookup = OneKey(StaticPubKey::from([0x07; 32]));
        let token_key = new_token_key().unwrap();

        let mut init = vec![0u8; INIT_LEN];
        let _ = init_payload(&mut init, &client, server.key_pair.public()).unwrap();

        serve(&init, &token_key, &server, &lookup)
            .await
            .unwrap_err();
    }

    #[test]
    fn tokens_are_rejected_when_tampered_with() {
        let key = new_token_key().unwrap();
        let other = new_token_key().unwrap();
        let static_pub = StaticPubKey::from([0x42; 32]);

        let token = issue_token(&key, static_pub);
        assert_eq!(verify_token(&key, &token).unwrap(), static_pub);

        // Signed by a different process.
        verify_token(&other, &token).unwrap_err();

        // Any flipped character breaks the MAC, including the key the token claims.
        let mut flipped: Vec<u8> = token.clone().into_bytes();
        flipped[0] = if flipped[0] == b'0' { b'1' } else { b'0' };
        verify_token(&key, std::str::from_utf8(&flipped).unwrap()).unwrap_err();

        verify_token(&key, &token[..token.len() - 2]).unwrap_err();
        verify_token(&key, "not hex").unwrap_err();
    }

    #[test]
    fn expired_tokens_are_rejected() {
        let key = new_token_key().unwrap();

        // Build one directly, because issue_token always dates into the future.
        let mut raw = [0u8; TOKEN_LEN];
        raw[..KEY_LEN].copy_from_slice(&[0x42; KEY_LEN]);
        raw[KEY_LEN..SIGNED_LEN].copy_from_slice(&1u64.to_be_bytes());
        let mac = hmac::sign(&key, &raw[..SIGNED_LEN]);
        raw[SIGNED_LEN..].copy_from_slice(mac.as_ref());

        let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();

        let err = format!("{:#}", verify_token(&key, &token).unwrap_err());
        assert!(err.contains("expired"), "{err}");
    }
}
