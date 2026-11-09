use super::*;
use rusqlite::OptionalExtension;
use shared::bee_msg::misc::*;
use shared::conn::beecrypt::KeyPairId;
use shared::conn::{IdentityId, PeerIdentity};

impl HandleWithResponse for RegisterIdentity {
    type Response = RegisterIdentityResp;

    fn error_response() -> Self::Response {
        RegisterIdentityResp {
            result: OpsErr::INTERNAL,
        }
    }

    async fn handle(self, app: &impl App, req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(app)?;

        let parsed = String::from_utf8(self.name).ok().zip(
            std::str::from_utf8(&self.key_id)
                .ok()
                .and_then(|s| s.parse::<KeyPairId>().ok()),
        );

        let Some((name, key_id)) = parsed.filter(|(name, _)| !name.is_empty()) else {
            log::warn!(
                "Peer {:?} tried to register an identity with an invalid name or key pair ID",
                req.addr()
            );
            return Ok(RegisterIdentityResp {
                result: OpsErr::INVAL,
            });
        };

        let identity_id: IdentityId = app
            .write_tx({
                let name = name.clone();
                move |tx| {
                    tx.execute_cached(
                        sql!("INSERT INTO identities (name) VALUES (?1) ON CONFLICT DO NOTHING"),
                        [&name],
                    )?;

                    let identity_id = tx.query_row_cached(
                        sql!("SELECT identity_id FROM identities WHERE name = ?1"),
                        [&name],
                        |row| row.get(0),
                    )?;

                    let key = key_id.to_string();
                    let existing: Option<IdentityId> = tx
                        .query_row_cached(
                            sql!("SELECT identity_id FROM keys WHERE key = ?1"),
                            [&key],
                            |row| row.get(0),
                        )
                        .optional()?;

                    match existing {
                        Some(id) if id == identity_id => {}
                        Some(_) => {
                            bail!("Key pair ID {key} is already registered to another identity")
                        }
                        None => {
                            tx.execute_cached(
                                sql!("INSERT INTO keys (key, identity_id) VALUES (?1, ?2)"),
                                rusqlite::params![key, identity_id],
                            )?;
                        }
                    }

                    Ok(identity_id)
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
}
