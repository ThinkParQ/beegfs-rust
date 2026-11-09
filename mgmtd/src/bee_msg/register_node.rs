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

        let node_type = self.node_type;
        let node_id = update_node(self, app, reject).await?;

        if let PeerIdentity::Registered {
            key_id,
            identity_id,
        } = req.peer_identity()
        {
            app.write_tx(move |tx| {
                tx.execute(
                    sql!("DELETE FROM identity_to_node WHERE node_type = ?1 AND node_id = ?2"),
                    rusqlite::params![node_type.sql_variant(), node_id],
                )?;
                tx.execute(
                    sql!(
                        "INSERT INTO identity_to_node (identity_id, node_type, node_id)
                        VALUES (?1, ?2, ?3)"
                    ),
                    rusqlite::params![identity_id, node_type.sql_variant(), node_id],
                )?;
                Ok(())
            })
            .await?;

            log::info!(
                "Mapped {node_type} node {node_id} to identity {identity_id} \
                (key pair ID {key_id})"
            );
        }

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

    async fn register_storage(app: &TestApp, req: &mut TestRequest, node_id: NodeId) -> NodeId {
        RegisterNode {
            node_alias: b"storage_new".to_vec(),
            node_type: NodeType::Storage,
            node_id,
            port: 8003,
            ..Default::default()
        }
        .handle(app, req)
        .await
        .unwrap()
        .node_num_id
    }

    async fn mapped_identities(app: &TestApp, node_id: NodeId) -> Vec<i64> {
        app.read_tx(move |tx| {
            Ok(tx.query_map_collect(
                sql!(
                    "SELECT identity_id FROM identity_to_node
                    WHERE node_type = ?1 AND node_id = ?2"
                ),
                rusqlite::params![NodeType::Storage.sql_variant(), node_id],
                |row| row.get(0),
            )?)
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn map_identity() {
        let app = TestApp::new().await;
        let mut req = TestRequest::new(Header::default());

        let node_id = register_storage(&app, &mut req, 0).await;
        assert_ne!(node_id, 0);
        assert!(mapped_identities(&app, node_id).await.is_empty());

        let mut ids = vec![];
        for (name, c) in [(b"id1", "a"), (b"id2", "b")] {
            let key_id: KeyPairId = format!("sha256:{}", c.repeat(64)).parse().unwrap();
            req.identity = PeerIdentity::Unregistered(key_id);
            RegisterIdentity {
                name: name.to_vec(),
                key_id: key_id.to_string().into_bytes(),
            }
            .handle(&app, &mut req)
            .await
            .unwrap();

            let PeerIdentity::Registered { identity_id, .. } = req.identity else {
                panic!("identity not registered");
            };
            ids.push(identity_id);

            assert_eq!(register_storage(&app, &mut req, node_id).await, node_id);
            assert_eq!(mapped_identities(&app, node_id).await, vec![identity_id]);
        }

        assert_ne!(ids[0], ids[1]);
    }
}
