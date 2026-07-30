use super::*;
use shared::bee_msg::misc::*;
use shared::crypto::handshake;

impl HandleWithResponse for KeyExchangeRequest {
    type Response = KeyExchangeResponse;

    /// Responder side of the BeeMsg key exchange.
    ///
    /// The peer's public key is looked up in the key list downloaded from the database - a key that
    /// is not on the list means the peer is rejected here, before any key agreement happens. That
    /// lookup *is* the authentication decision; the handshake itself then proves the peer actually
    /// holds the matching private key.
    async fn handle(self, app: &impl App, req: &mut impl Request) -> Result<Self::Response> {
        let Some(keypair) = app.static_info().static_keypair.as_ref() else {
            bail!(
                "Peer {:?} attempted a key exchange, but authentication is disabled on this node",
                req.addr()
            );
        };

        if self.version != handshake::HANDSHAKE_VERSION {
            bail!(
                "Peer {:?} requested unsupported key exchange version {} (we support {})",
                req.addr(),
                self.version,
                handshake::HANDSHAKE_VERSION
            );
        }

        // This is the authentication decision.
        let Some(node_uid) = app.key_store().node_by_key(&self.static_pub) else {
            bail!(
                "Peer {:?} presented public key {} which is not registered - rejecting",
                req.addr(),
                self.static_pub
            );
        };

        let response = handshake::respond(keypair, self.static_pub, &self.ephemeral_pub)
            .with_context(|| {
                format!(
                    "Key exchange with peer {:?} (node uid {node_uid}) failed",
                    req.addr()
                )
            })?;

        log::debug!(
            "Key exchange with {:?} succeeded, authenticated as node uid {node_uid}",
            req.addr()
        );

        // The response below is the last plaintext message on this stream, so the session must not
        // take effect until it has been written - `Request::install_session` stages it for exactly
        // that (see `Stream::stage_session`). Activation also resets the message counters, so the
        // first encrypted message in each direction uses counter 0.
        req.install_session(response.session, self.static_pub);

        Ok(KeyExchangeResponse {
            ephemeral_pub: response.ephemeral_pub,
            confirm: response.confirm,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::app::test::*;
    use shared::bee_msg::Header;
    use shared::crypto::handshake::StaticKeypair;
    use shared::types::StaticPubKey;

    /// An unregistered public key must be rejected before any key agreement happens.
    #[tokio::test]
    async fn unregistered_key_is_rejected() {
        let app = TestApp::new().await;
        let mut req = TestRequest::new(Header::default());

        let initiator = StaticKeypair::generate();

        let err = KeyExchangeRequest {
            version: handshake::HANDSHAKE_VERSION,
            static_pub: initiator.public(),
            ephemeral_pub: [1u8; 32],
        }
        .handle(&app, &mut req)
        .await
        .expect_err("an unregistered key must not be accepted");

        assert!(err.to_string().contains("not registered"));
        assert!(
            req.installed_session_peer.is_none(),
            "no session may be installed for a rejected peer"
        );
    }

    /// A version the responder does not implement must be rejected rather than guessed at.
    #[tokio::test]
    async fn unsupported_version_is_rejected() {
        let app = TestApp::new().await;
        let mut req = TestRequest::new(Header::default());

        let initiator = StaticKeypair::generate();
        app.key_store().insert(1, initiator.public());

        let err = KeyExchangeRequest {
            version: handshake::HANDSHAKE_VERSION + 1,
            static_pub: initiator.public(),
            ephemeral_pub: [1u8; 32],
        }
        .handle(&app, &mut req)
        .await
        .expect_err("an unsupported version must not be accepted");

        assert!(err.to_string().contains("unsupported key exchange version"));
    }

    /// A registered peer completes the handshake, and the initiator half must be able to finish it
    /// from the returned response - proving both sides derived the same keys.
    #[tokio::test]
    async fn registered_key_completes_handshake() {
        let initiator = StaticKeypair::generate();

        let app = TestApp::new().await;
        app.key_store().insert(1, initiator.public());
        let responder_pub = app
            .static_info()
            .static_keypair
            .as_ref()
            .expect("test app must have a keypair")
            .public();

        let mut req = TestRequest::new(Header::default());

        let init = handshake::Initiator::start(&initiator, responder_pub).unwrap();

        let resp = KeyExchangeRequest {
            version: handshake::HANDSHAKE_VERSION,
            static_pub: initiator.public(),
            ephemeral_pub: init.ephemeral_pub(),
        }
        .handle(&app, &mut req)
        .await
        .unwrap();

        // The initiator accepting the confirmation proves both sides agree on the transcript and
        // therefore on the session keys.
        init.finish(&resp.ephemeral_pub, &resp.confirm)
            .expect("initiator must accept the responders confirmation");

        assert_eq!(
            req.installed_session_peer,
            Some(StaticPubKey::from(*initiator.public().as_bytes())),
            "the session must be installed against the peers key"
        );
    }
}
