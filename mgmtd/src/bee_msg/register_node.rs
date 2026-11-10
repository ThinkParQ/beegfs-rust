use super::*;
use common::{NodeRegistration, update_node};
use shared::bee_msg::node::*;

const COMPATFLAG_CLIENT_SUPPORTS_REGREJ: u8 = 1;

impl HandleWithResponse for RegisterNode {
    type Response = RegisterNodeResp;

    async fn handle(self, app: &impl App, req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(app)?;

        if app.static_info().conn_crypto.is_some() {
            bail!("RegisterNode requires the legacy connection protocol, use RegisterNodeV2");
        }

        let reject =
            (req.header().msg_compat_feature_flags & COMPATFLAG_CLIENT_SUPPORTS_REGREJ) != 0;

        let NodeRegistration::Updated(update) = update_node(self, app, reject, None).await? else {
            bail!("Legacy node registration did not complete");
        };

        let fs_uuid: String = app
            .read_tx(|tx| db::config::get(tx, db::config::Config::FsUuid))
            .await?
            .ok_or_else(|| anyhow!("Could not read file system UUID from database"))?;

        Ok(RegisterNodeResp {
            node_num_id: update.node_id,
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

    fn msg() -> RegisterNode {
        RegisterNode {
            node_alias: b"storage_legacy".to_vec(),
            node_type: NodeType::Storage,
            port: 8003,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn register_node() {
        let app = TestApp::new().await;
        let mut req = TestRequest::new(Header::default());
        let resp = msg().handle(&app, &mut req).await.unwrap();
        assert_ne!(resp.node_num_id, 0);

        let app = TestApp::non_legacy().await;
        msg().handle(&app, &mut req).await.unwrap_err();
    }
}
