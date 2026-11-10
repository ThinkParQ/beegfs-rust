use super::*;
use common::{NodeRegistration, PeerKey, update_node};
use shared::bee_msg::misc::RegistrationStatus;
use shared::bee_msg::node::*;
use shared::conn::PeerIdentity;

const COMPATFLAG_CLIENT_SUPPORTS_REGREJ: u8 = 1;

impl HandleWithResponse for RegisterNodeV2 {
    type Response = RegisterNodeV2Resp;

    fn error_response() -> Self::Response {
        RegisterNodeV2Resp {
            status: RegistrationStatus::Rejected,
            message: b"Internal error, see the management log".to_vec(),
            ..Default::default()
        }
    }

    async fn handle(self, app: &impl App, req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(app)?;

        let key_id = match req.peer_identity() {
            PeerIdentity::Unregistered(key_id) | PeerIdentity::Registered { key_id, .. } => key_id,
            PeerIdentity::Legacy => {
                bail!("RegisterNodeV2 requires a non-legacy connection protocol")
            }
        };

        let reject =
            (req.header().msg_compat_feature_flags & COMPATFLAG_CLIENT_SUPPORTS_REGREJ) != 0;

        let token = match String::from_utf8(self.identity_token) {
            Ok(token) => Some(token).filter(|t| !t.is_empty()),
            Err(_) => {
                return Ok(RegisterNodeV2Resp {
                    status: RegistrationStatus::Rejected,
                    message: b"Malformed identity token".to_vec(),
                    ..Default::default()
                });
            }
        };

        let peer_key = PeerKey {
            key_id,
            token,
            addr: req.addr().to_string(),
        };

        let update = match update_node(self.node, app, reject, Some(peer_key)).await? {
            NodeRegistration::Updated(update) => update,
            NodeRegistration::Pending => {
                log::info!(
                    "Key pair ID {key_id} from {:?} is pending approval",
                    req.addr()
                );
                return Ok(RegisterNodeV2Resp {
                    status: RegistrationStatus::Pending,
                    message: format!("Key pair ID {key_id} is pending approval by an admin")
                        .into_bytes(),
                    ..Default::default()
                });
            }
            NodeRegistration::Rejected(reason) => {
                log::warn!(
                    "Rejected key pair ID {key_id} from {:?}: {reason}",
                    req.addr()
                );
                return Ok(RegisterNodeV2Resp {
                    status: RegistrationStatus::Rejected,
                    message: reason.into_bytes(),
                    ..Default::default()
                });
            }
        };

        if let Some(identity_id) = update.identity_id {
            req.set_peer_identity(PeerIdentity::Registered {
                key_id,
                identity_id,
            });
        }

        if update.identities_changed {
            notify_identities_changed(app, update.identities_version).await;
        }

        let fs_uuid: String = app
            .read_tx(|tx| db::config::get(tx, db::config::Config::FsUuid))
            .await?
            .ok_or_else(|| anyhow!("Could not read file system UUID from database"))?;

        Ok(RegisterNodeV2Resp {
            status: RegistrationStatus::Ok,
            message: vec![],
            node_num_id: update.node_id,
            grpc_port: app.static_info().user_config.grpc_port,
            fs_uuid: fs_uuid.into_bytes(),
            identities_version: update.identities_version,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::app::test::*;
    use shared::bee_msg::Header;
    use shared::bee_msg::misc::{RefreshIdentities, RegisterIdentity};
    use shared::conn::beecrypt::KeyPairId;

    fn msg(node_id: NodeId) -> RegisterNodeV2 {
        RegisterNodeV2 {
            node: RegisterNode {
                node_alias: b"storage_v2".to_vec(),
                node_type: NodeType::Storage,
                node_id,
                port: 8003,
                ..Default::default()
            },
            identity_token: vec![],
        }
    }

    fn client_msg(node_id: NodeId, token: &str) -> RegisterNodeV2 {
        RegisterNodeV2 {
            node: RegisterNode {
                node_alias: b"client_v2".to_vec(),
                node_type: NodeType::Client,
                node_id,
                port: 8004,
                ..Default::default()
            },
            identity_token: token.as_bytes().to_vec(),
        }
    }

    #[tokio::test]
    async fn register_node_v2() {
        let app = TestApp::new().await;
        let key_id: KeyPairId = format!("sha256:{}", "a".repeat(64)).parse().unwrap();
        let mut req = TestRequest::new(Header::default());
        req.identity = PeerIdentity::Unregistered(key_id);

        let resp = msg(0).handle(&app, &mut req).await.unwrap();
        assert_ne!(resp.node_num_id, 0);
        assert_eq!(resp.identities_version, 2);
        assert!(matches!(
            req.identity,
            PeerIdentity::Registered { key_id: k, .. } if k == key_id
        ));
        assert!(app.has_sent_notification::<RefreshIdentities>(&[
            NodeType::Meta,
            NodeType::Storage,
            NodeType::Client
        ]));

        let again = msg(resp.node_num_id).handle(&app, &mut req).await.unwrap();
        assert_eq!(again.node_num_id, resp.node_num_id);
        assert_eq!(again.identities_version, resp.identities_version);

        let other: KeyPairId = format!("sha256:{}", "b".repeat(64)).parse().unwrap();
        let mut other_req = TestRequest::new(Header::default());
        other_req.identity = PeerIdentity::Unregistered(other);
        msg(resp.node_num_id)
            .handle(&app, &mut other_req)
            .await
            .unwrap_err();

        let mut legacy_req = TestRequest::new(Header::default());
        msg(0).handle(&app, &mut legacy_req).await.unwrap_err();

        let version_after_delete = app
            .write_tx(move |tx| {
                tx.execute(
                    sql!("DELETE FROM identity_to_node WHERE node_id = ?1"),
                    [resp.node_num_id],
                )?;
                crate::bee_msg::identities_version(tx)
            })
            .await
            .unwrap();
        assert_eq!(version_after_delete, resp.identities_version + 1);
    }

    async fn register_identity(app: &TestApp, name: &str, c: &str) -> (KeyPairId, i64) {
        let key_id: KeyPairId = format!("sha256:{}", c.repeat(64)).parse().unwrap();
        let mut req = TestRequest::new(Header::default());
        req.identity = PeerIdentity::Unregistered(key_id);
        RegisterIdentity {
            name: name.as_bytes().to_vec(),
            ..Default::default()
        }
        .handle(app, &mut req)
        .await
        .unwrap();

        let PeerIdentity::Registered { identity_id, .. } = req.identity else {
            panic!("identity not registered");
        };
        (key_id, identity_id)
    }

    async fn register_v2(app: &TestApp, key_id: KeyPairId, node_id: NodeId) -> Result<NodeId> {
        let mut req = TestRequest::new(Header::default());
        req.identity = PeerIdentity::Registered {
            key_id,
            identity_id: 0,
        };
        let resp = msg(node_id).handle(app, &mut req).await?;
        if resp.status != RegistrationStatus::Ok {
            bail!(
                "{:?}: {}",
                resp.status,
                String::from_utf8_lossy(&resp.message)
            );
        }
        Ok(resp.node_num_id)
    }

    async fn mapping(app: &TestApp) -> Vec<(i64, NodeId)> {
        app.read_tx(|tx| {
            Ok(tx.query_map_collect(
                sql!("SELECT identity_id, node_id FROM identity_to_node ORDER BY identity_id"),
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?)
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn map_identity() {
        let app = TestApp::new().await;

        let legacy_node = RegisterNode {
            node_alias: b"legacy".to_vec(),
            node_type: NodeType::Storage,
            port: 8003,
            ..Default::default()
        }
        .handle(&app, &mut TestRequest::new(Header::default()))
        .await
        .unwrap()
        .node_num_id;
        assert!(mapping(&app).await.is_empty());

        let (key_a, id1) = register_identity(&app, "id1", "a").await;
        let node1 = register_v2(&app, key_a, 0).await.unwrap();
        assert_eq!(mapping(&app).await, vec![(id1, node1)]);

        let (key_b, id_b) = register_identity(&app, "id1", "b").await;
        assert_ne!(id_b, id1);
        register_v2(&app, key_b, node1).await.unwrap_err();

        register_v2(&app, key_a, 0).await.unwrap_err();
        register_v2(&app, key_a, legacy_node).await.unwrap_err();

        let (key_c, _) = register_identity(&app, "id2", "c").await;
        register_v2(&app, key_c, node1).await.unwrap_err();

        assert_eq!(mapping(&app).await, vec![(id1, node1)]);
    }

    async fn pending_keys(app: &TestApp) -> i64 {
        app.read_tx(|tx| {
            Ok(
                tx.query_row(sql!("SELECT COUNT(*) FROM pending_keys"), [], |row| {
                    row.get(0)
                })?,
            )
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn server_approval() {
        let app = TestApp::non_legacy_with_config(crate::config::Config {
            register_server_mode: crate::config::RegisterServerMode::Approval,
            ..Default::default()
        })
        .await;
        let key_id: KeyPairId = format!("sha256:{}", "a".repeat(64)).parse().unwrap();
        let mut req = TestRequest::new(Header::default());
        req.identity = PeerIdentity::Unregistered(key_id);

        let resp = msg(0).handle(&app, &mut req).await.unwrap();
        assert_eq!(resp.status, RegistrationStatus::Pending);
        assert_eq!(resp.node_num_id, 0);
        assert_eq!(pending_keys(&app).await, 1);
        assert!(mapping(&app).await.is_empty());

        app.write_tx(move |tx| {
            crate::bee_msg::identity_admission::approve_pending_key(tx, &key_id)
        })
        .await
        .unwrap();

        let resp = msg(0).handle(&app, &mut req).await.unwrap();
        assert_eq!(resp.status, RegistrationStatus::Ok);
        assert_ne!(resp.node_num_id, 0);
        assert_eq!(pending_keys(&app).await, 0);
        assert_eq!(mapping(&app).await.len(), 1);
    }

    #[tokio::test]
    async fn servers_disabled() {
        let app = TestApp::non_legacy_with_config(crate::config::Config {
            register_server_mode: crate::config::RegisterServerMode::Disabled,
            ..Default::default()
        })
        .await;
        let key_id: KeyPairId = format!("sha256:{}", "a".repeat(64)).parse().unwrap();
        let mut req = TestRequest::new(Header::default());
        req.identity = PeerIdentity::Unregistered(key_id);

        let resp = msg(0).handle(&app, &mut req).await.unwrap();
        assert_eq!(resp.status, RegistrationStatus::Rejected);
        assert!(mapping(&app).await.is_empty());
        assert_eq!(pending_keys(&app).await, 0);

        shared::bee_msg::target::RegisterTarget {
            reg_token: b"new_target".to_vec(),
            target_id: 0,
        }
        .handle(&app, &mut TestRequest::new(Header::default()))
        .await
        .unwrap_err();
    }

    #[tokio::test]
    async fn client_token() {
        let app = TestApp::non_legacy_with_config(crate::config::Config {
            register_client_mode: crate::config::RegisterClientMode::Token,
            ..Default::default()
        })
        .await;
        let token = app
            .write_tx(|tx| {
                crate::bee_msg::create_identity_token(
                    tx,
                    crate::config::TokenScope::Client,
                    false,
                    None,
                )
            })
            .await
            .unwrap();

        let register = |c: &str, token: &str| {
            let key_id: KeyPairId = format!("sha256:{}", c.repeat(64)).parse().unwrap();
            let mut req = TestRequest::new(Header::default());
            req.identity = PeerIdentity::Unregistered(key_id);
            let msg = client_msg(0, token);
            let app = &app;
            async move { msg.handle(app, &mut req).await.unwrap() }
        };

        assert_eq!(register("a", "").await.status, RegistrationStatus::Rejected);

        let resp = register("a", &token).await;
        assert_eq!(resp.status, RegistrationStatus::Ok);
        assert_ne!(resp.node_num_id, 0);

        let resp = register("b", &token).await;
        assert_eq!(resp.status, RegistrationStatus::Rejected);
        assert!(String::from_utf8_lossy(&resp.message).contains("already been used"));
    }

    #[tokio::test]
    async fn replace_stale_client() {
        let app = TestApp::new().await;
        let key_id: KeyPairId = format!("sha256:{}", "a".repeat(64)).parse().unwrap();

        let register = |node_id| {
            let app = &app;
            async move {
                let mut req = TestRequest::new(Header::default());
                req.identity = PeerIdentity::Unregistered(key_id);
                client_msg(node_id, "")
                    .handle(app, &mut req)
                    .await
                    .unwrap()
                    .node_num_id
            }
        };

        let client_exists = |node_id: NodeId| {
            app.read_tx(move |tx| {
                Ok(tx.query_row(
                    sql!("SELECT COUNT(*) FROM nodes WHERE node_type = ?1 AND node_id = ?2"),
                    rusqlite::params![NodeType::Client.sql_variant(), node_id],
                    |row| row.get::<_, i64>(0),
                )? == 1)
            })
        };

        let first = register(0).await;
        let (identity_id, _) = mapping(&app).await[0];
        assert_eq!(register(first).await, first);

        let second = register(0).await;
        assert_ne!(second, first);
        assert!(!client_exists(first).await.unwrap());
        assert!(client_exists(second).await.unwrap());
        assert_eq!(mapping(&app).await, vec![(identity_id, second)]);
    }
}
