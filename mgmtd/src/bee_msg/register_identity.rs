use super::*;
use rusqlite::Transaction;
use shared::bee_msg::misc::*;
use shared::conn::PeerIdentity;

pub(crate) fn identities_version(tx: &Transaction) -> Result<u64> {
    Ok(db::config::get(tx, db::config::Config::IdentitiesVersion)?.unwrap_or(0))
}

pub(crate) async fn notify_identities_changed(app: &impl App, version: u64) {
    log::info!("Identities changed, notifying nodes about version {version}");

    app.send_notifications(
        &[NodeType::Meta, NodeType::Storage, NodeType::Client],
        &RefreshIdentities { version },
    )
    .await;
}

impl HandleWithResponse for RegisterIdentity {
    type Response = RegisterIdentityResp;

    fn error_response() -> Self::Response {
        rejected("Internal error, see the management log".to_string())
    }

    async fn handle(self, app: &impl App, req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(app)?;

        let key_id = match req.peer_identity() {
            PeerIdentity::Unregistered(key_id) | PeerIdentity::Registered { key_id, .. } => key_id,
            PeerIdentity::Legacy => {
                bail!("RegisterIdentity requires a non-legacy connection protocol")
            }
        };

        let (Ok(name), Ok(token)) = (
            String::from_utf8(self.name),
            String::from_utf8(self.identity_token),
        ) else {
            return Ok(rejected("Malformed name or identity token".to_string()));
        };

        let key_req = KeyRequest {
            key_id,
            category: match self.category {
                IdentityCategory::Client => KeyCategory::Client,
                IdentityCategory::Server => KeyCategory::Server,
            },
            token: Some(token).filter(|t| !t.is_empty()),
            node: None,
            name: Some(name).filter(|name| !name.is_empty()),
            addr: req.addr().to_string(),
        };
        let modes = AdmissionModes::from_config(&app.static_info().user_config);

        let (admission, version_before, version) = app
            .write_tx({
                let key_req = key_req.clone();
                move |tx| {
                    let version_before = identities_version(tx)?;
                    let admission = admit_key(tx, modes, &key_req)?;
                    Ok((admission, version_before, identities_version(tx)?))
                }
            })
            .await?;

        let (identity_id, known) = match admission {
            Admission::Known(identity_id) => (identity_id, true),
            Admission::Registered(identity_id) => (identity_id, false),
            Admission::Pending => {
                log::info!(
                    "Key pair ID {key_id} from {:?} is pending approval",
                    req.addr()
                );
                return Ok(RegisterIdentityResp {
                    status: RegistrationStatus::Pending,
                    message: format!("Key pair ID {key_id} is pending approval by an admin")
                        .into_bytes(),
                });
            }
            Admission::Rejected(reason) => {
                log::warn!(
                    "Rejected key pair ID {key_id} from {:?}: {reason}",
                    req.addr()
                );
                return Ok(rejected(reason));
            }
        };

        if known {
            log::debug!(
                "Key pair ID {key_id} from {:?} is already registered as identity {identity_id}",
                req.addr()
            );
        } else {
            log::info!(
                "Registered key pair ID {key_id} for identity {:?} (ID {identity_id}) from {:?}",
                key_req.name,
                req.addr()
            );
        }

        req.set_peer_identity(PeerIdentity::Registered {
            key_id,
            identity_id,
        });

        if version != version_before {
            notify_identities_changed(app, version).await;
        }

        Ok(RegisterIdentityResp {
            status: RegistrationStatus::Ok,
            message: vec![],
        })
    }
}

fn rejected(reason: String) -> RegisterIdentityResp {
    RegisterIdentityResp {
        status: RegistrationStatus::Rejected,
        message: reason.into_bytes(),
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::app::test::*;
    use shared::bee_msg::Header;
    use shared::conn::beecrypt::KeyPairId;

    fn msg(name: &str, token: &str) -> RegisterIdentity {
        RegisterIdentity {
            category: IdentityCategory::Client,
            name: name.as_bytes().to_vec(),
            identity_token: token.as_bytes().to_vec(),
        }
    }

    #[tokio::test]
    async fn register_identity() {
        let app = TestApp::new().await;
        let key_id: KeyPairId = format!("sha256:{}", "a".repeat(64)).parse().unwrap();

        let mut req = TestRequest::new(Header::default());
        req.identity = PeerIdentity::Unregistered(key_id);

        let resp = msg("node1", "").handle(&app, &mut req).await.unwrap();
        assert_eq!(resp.status, RegistrationStatus::Ok);
        assert!(matches!(
            req.identity,
            PeerIdentity::Registered { key_id: k, .. } if k == key_id
        ));

        let resp = msg("node2", "ignored")
            .handle(&app, &mut req)
            .await
            .unwrap();
        assert_eq!(resp.status, RegistrationStatus::Ok);

        let other: KeyPairId = format!("sha256:{}", "b".repeat(64)).parse().unwrap();
        let mut other_req = TestRequest::new(Header::default());
        other_req.identity = PeerIdentity::Unregistered(other);
        let resp = msg("node3", "beegfs-idt-1.1.00")
            .handle(&app, &mut other_req)
            .await
            .unwrap();
        assert_eq!(resp.status, RegistrationStatus::Rejected);
        assert_eq!(other_req.identity, PeerIdentity::Unregistered(other));

        let mut legacy_req = TestRequest::new(Header::default());
        msg("node4", "")
            .handle(&app, &mut legacy_req)
            .await
            .unwrap_err();
    }

    #[tokio::test]
    async fn register_anonymous() {
        let app = TestApp::new().await;
        let mut ids = vec![];

        for c in ["a", "a", "b"] {
            let key_id: KeyPairId = format!("sha256:{}", c.repeat(64)).parse().unwrap();
            let mut req = TestRequest::new(Header::default());
            req.identity = PeerIdentity::Unregistered(key_id);

            let resp = msg("", "").handle(&app, &mut req).await.unwrap();
            assert_eq!(resp.status, RegistrationStatus::Ok);

            let PeerIdentity::Registered { identity_id, .. } = req.identity else {
                panic!("identity not registered");
            };
            ids.push(identity_id);
        }

        assert_eq!(ids[0], ids[1]);
        assert_ne!(ids[0], ids[2]);
    }
}
