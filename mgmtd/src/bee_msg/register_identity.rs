use super::*;
use rusqlite::{OptionalExtension, Transaction};
use shared::bee_msg::misc::*;
use shared::conn::beecrypt::KeyPairId;
use shared::conn::{IdentityId, PeerIdentity};

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

pub(crate) fn register_key(
    tx: &Transaction,
    name: Option<String>,
    key_id: &KeyPairId,
) -> Result<IdentityId> {
    let key = key_id.to_string();
    let existing: Option<IdentityId> = tx
        .query_row_cached(
            sql!("SELECT identity_id FROM keys WHERE key = ?1"),
            [&key],
            |row| row.get(0),
        )
        .optional()?;

    let identity_id = match (name, existing) {
        (None, Some(id)) => return Ok(id),
        (None, None) => {
            tx.execute_cached(sql!("INSERT INTO identities (name) VALUES (NULL)"), [])?;
            tx.last_insert_rowid()
        }
        (Some(name), _) => {
            tx.execute_cached(
                sql!("INSERT INTO identities (name) VALUES (?1) ON CONFLICT DO NOTHING"),
                [&name],
            )?;
            tx.query_row_cached(
                sql!("SELECT identity_id FROM identities WHERE name = ?1"),
                [&name],
                |row| row.get(0),
            )?
        }
    };

    match existing {
        Some(id) if id == identity_id => {}
        Some(_) => bail!("Key pair ID {key} is already registered to another identity"),
        None => {
            tx.execute_cached(
                sql!("INSERT INTO keys (key, identity_id) VALUES (?1, ?2)"),
                rusqlite::params![key, identity_id],
            )?;
        }
    }

    Ok(identity_id)
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

        let name = Some(name).filter(|name| !name.is_empty());

        let (identity_id, version_before, version) = app
            .write_tx({
                let name = name.clone();
                move |tx| {
                    let version_before = identities_version(tx)?;
                    let identity_id = register_key(tx, name, &key_id)?;
                    Ok((identity_id, version_before, identities_version(tx)?))
                }
            })
            .await?;

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

        RegisterIdentity {
            name: b"node2".to_vec(),
            key_id: key_id.to_string().into_bytes(),
        }
        .handle(&app, &mut req)
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
