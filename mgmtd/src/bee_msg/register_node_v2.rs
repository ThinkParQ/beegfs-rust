use super::*;
use common::update_node;
use shared::bee_msg::node::*;
use shared::conn::PeerIdentity;

const COMPATFLAG_CLIENT_SUPPORTS_REGREJ: u8 = 1;

impl HandleWithResponse for RegisterNodeV2 {
    type Response = RegisterNodeV2Resp;

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

        let update = update_node(self.0, app, reject, Some(key_id)).await?;

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
        RegisterNodeV2(RegisterNode {
            node_alias: b"storage_v2".to_vec(),
            node_type: NodeType::Storage,
            node_id,
            port: 8003,
            ..Default::default()
        })
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
            key_id: key_id.to_string().into_bytes(),
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
        Ok(msg(node_id).handle(app, &mut req).await?.node_num_id)
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

        let (key_b, id1_again) = register_identity(&app, "id1", "b").await;
        assert_eq!(id1_again, id1);
        assert_eq!(register_v2(&app, key_b, node1).await.unwrap(), node1);

        register_v2(&app, key_a, 0).await.unwrap_err();
        register_v2(&app, key_a, legacy_node).await.unwrap_err();

        let (key_c, _) = register_identity(&app, "id2", "c").await;
        register_v2(&app, key_c, node1).await.unwrap_err();

        assert_eq!(mapping(&app).await, vec![(id1, node1)]);
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
                RegisterNodeV2(RegisterNode {
                    node_alias: b"client_v2".to_vec(),
                    node_type: NodeType::Client,
                    node_id,
                    port: 8004,
                    ..Default::default()
                })
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
