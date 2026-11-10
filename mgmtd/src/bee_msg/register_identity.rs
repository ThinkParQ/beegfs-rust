use super::*;
use rusqlite::Transaction;
use shared::bee_msg::misc::*;
use shared::conn::PeerIdentity;
use shared::conn::beecrypt::KeyPairId;

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
        RegisterIdentityResp {
            result: OpsErr::INTERNAL,
        }
    }

    async fn handle(self, app: &impl App, req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(app)?;

        if req.peer_identity() == PeerIdentity::Legacy {
            bail!("RegisterIdentity requires a non-legacy connection protocol");
        }

        let parsed = String::from_utf8(self.name).ok().zip(
            std::str::from_utf8(&self.key_id)
                .ok()
                .and_then(|s| s.parse::<KeyPairId>().ok()),
        );

        let Some((name, key_id)) = parsed else {
            log::warn!(
                "Peer {:?} tried to register an identity with an invalid name or key pair ID",
                req.addr()
            );
            return Ok(RegisterIdentityResp {
                result: OpsErr::INVAL,
            });
        };

        let key_req = KeyRequest {
            key_id,
            category: KeyCategory::Client,
            token: None,
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

        let identity_id = match admission {
            Admission::Registered(identity_id) => identity_id,
            Admission::Pending => {
                log::info!(
                    "Key pair ID {key_id} from {:?} is pending approval",
                    req.addr()
                );
                return Ok(RegisterIdentityResp {
                    result: OpsErr::AGAIN,
                });
            }
            Admission::Rejected(reason) => {
                log::warn!(
                    "Rejected key pair ID {key_id} from {:?}: {reason}",
                    req.addr()
                );
                return Ok(RegisterIdentityResp {
                    result: OpsErr::INVAL,
                });
            }
        };
        let name = key_req.name;

        log::info!(
            "Registered key pair ID {key_id} for identity {name:?} (ID {identity_id}) from {:?}",
            req.addr()
        );

        if req.peer_identity() == PeerIdentity::Unregistered(key_id) {
            req.set_peer_identity(PeerIdentity::Registered {
                key_id,
                identity_id,
            });
        }

        if version != version_before {
            notify_identities_changed(app, version).await;
        }

        Ok(RegisterIdentityResp {
            result: OpsErr::SUCCESS,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::app::test::*;
    use shared::bee_msg::Header;

    #[tokio::test]
    async fn register_identity() {
        let app = TestApp::new().await;
        let key_id: KeyPairId = format!("sha256:{}", "a".repeat(64)).parse().unwrap();

        let mut req = TestRequest::new(Header::default());
        req.identity = PeerIdentity::Unregistered(key_id);

        let resp = RegisterIdentity {
            name: b"node1".to_vec(),
            key_id: key_id.to_string().into_bytes(),
        }
        .handle(&app, &mut req)
        .await
        .unwrap();

        assert_eq!(resp.result, OpsErr::SUCCESS);
        assert!(matches!(
            req.identity,
            PeerIdentity::Registered { key_id: k, .. } if k == key_id
        ));

        let resp = RegisterIdentity {
            name: b"node1".to_vec(),
            key_id: b"invalid".to_vec(),
        }
        .handle(&app, &mut req)
        .await
        .unwrap();

        assert_eq!(resp.result, OpsErr::INVAL);

        let resp = RegisterIdentity {
            name: b"node2".to_vec(),
            key_id: key_id.to_string().into_bytes(),
        }
        .handle(&app, &mut req)
        .await
        .unwrap();
        assert_eq!(resp.result, OpsErr::SUCCESS);
    }

    #[tokio::test]
    async fn register_anonymous() {
        let app = TestApp::new().await;
        let mut ids = vec![];

        for c in ["a", "a", "b"] {
            let key_id: KeyPairId = format!("sha256:{}", c.repeat(64)).parse().unwrap();
            let mut req = TestRequest::new(Header::default());
            req.identity = PeerIdentity::Unregistered(key_id);

            let resp = RegisterIdentity {
                name: vec![],
                key_id: key_id.to_string().into_bytes(),
            }
            .handle(&app, &mut req)
            .await
            .unwrap();
            assert_eq!(resp.result, OpsErr::SUCCESS);

            let PeerIdentity::Registered { identity_id, .. } = req.identity else {
                panic!("identity not registered");
            };
            ids.push(identity_id);
        }

        assert_eq!(ids[0], ids[1]);
        assert_ne!(ids[0], ids[2]);
    }
}
