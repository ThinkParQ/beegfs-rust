use super::*;
use common::update_node;
use shared::bee_msg::node::*;
use shared::conn::PeerIdentity;

const COMPATFLAG_CLIENT_SUPPORTS_REGREJ: u8 = 1;

impl HandleWithResponse for RegisterNode {
    type Response = RegisterNodeResp;

    async fn handle(self, app: &impl App, req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(app)?;

        let reject =
            (req.header().msg_compat_feature_flags & COMPATFLAG_CLIENT_SUPPORTS_REGREJ) != 0;

        let identity_id = match req.peer_identity() {
            PeerIdentity::Registered { identity_id, .. } => Some(identity_id),
            _ => None,
        };

        let node_id = update_node(self, app, reject, identity_id).await?;

        let fs_uuid: String = app
            .read_tx(|tx| db::config::get(tx, db::config::Config::FsUuid))
            .await?
            .ok_or_else(|| anyhow!("Could not read file system UUID from database"))?;

        Ok(RegisterNodeResp {
            node_num_id: node_id,
            grpc_port: app.static_info().user_config.grpc_port,
            fs_uuid: fs_uuid.into_bytes(),
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::app::test::*;
    use shared::bee_msg::Header;
    use shared::bee_msg::misc::RegisterIdentity;
    use shared::conn::beecrypt::KeyPairId;

    async fn register_identity(app: &TestApp, req: &mut TestRequest, name: &str, c: &str) -> i64 {
        let key_id: KeyPairId = format!("sha256:{}", c.repeat(64)).parse().unwrap();
        req.identity = PeerIdentity::Unregistered(key_id);
        RegisterIdentity {
            name: name.as_bytes().to_vec(),
            key_id: key_id.to_string().into_bytes(),
        }
        .handle(app, req)
        .await
        .unwrap();

        let PeerIdentity::Registered { identity_id, .. } = req.identity else {
            panic!("identity not registered");
        };
        identity_id
    }

    async fn register_storage(
        app: &TestApp,
        req: &mut TestRequest,
        alias: &str,
        node_id: NodeId,
    ) -> Result<NodeId> {
        Ok(RegisterNode {
            node_alias: alias.as_bytes().to_vec(),
            node_type: NodeType::Storage,
            node_id,
            port: 8003,
            ..Default::default()
        }
        .handle(app, req)
        .await?
        .node_num_id)
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
        let mut req = TestRequest::new(Header::default());

        let legacy_node = register_storage(&app, &mut req, "legacy", 0).await.unwrap();
        assert!(mapping(&app).await.is_empty());

        let id1 = register_identity(&app, &mut req, "id1", "a").await;
        let node1 = register_storage(&app, &mut req, "node1", 0).await.unwrap();
        assert_eq!(mapping(&app).await, vec![(id1, node1)]);

        register_identity(&app, &mut req, "id1", "b").await;
        assert_eq!(
            register_storage(&app, &mut req, "node1", node1)
                .await
                .unwrap(),
            node1
        );

        register_storage(&app, &mut req, "node2", 0)
            .await
            .unwrap_err();
        register_storage(&app, &mut req, "legacy", legacy_node)
            .await
            .unwrap_err();

        register_identity(&app, &mut req, "id2", "c").await;
        register_storage(&app, &mut req, "node1", node1)
            .await
            .unwrap_err();

        assert_eq!(mapping(&app).await, vec![(id1, node1)]);
    }
}
