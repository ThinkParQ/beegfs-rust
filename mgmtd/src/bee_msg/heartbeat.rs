use super::*;
use common::update_node;
use shared::bee_msg::misc::Ack;
use shared::bee_msg::node::*;

impl HandleWithResponse for Heartbeat {
    type Response = Ack;

    async fn handle(self, app: &impl App, _req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(app)?;

        if app.static_info().conn_crypto.is_some() {
            bail!("Heartbeat requires the legacy connection protocol, use RegisterNodeV2");
        }

        update_node(
            RegisterNode {
                instance_version: self.instance_version,
                nic_list_version: self.nic_list_version,
                node_alias: self.node_alias,
                nics: self.nic_list,
                node_type: self.node_type,
                node_id: self.node_num_id,
                root_num_id: self.root_num_id,
                is_root_mirrored: self.is_root_mirrored,
                port: self.port,
                port_tcp_unused: self.port_tcp_unused,
                machine_uuid: self.machine_uuid,
            },
            app,
            false,
            None,
        )
        .await?;

        Ok(Ack {
            ack_id: self.ack_id,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::app::test::*;
    use shared::bee_msg::Header;

    fn msg() -> Heartbeat {
        Heartbeat {
            node_alias: b"storage_legacy".to_vec(),
            node_type: NodeType::Storage,
            port: 8003,
            ack_id: b"ack".to_vec(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn heartbeat() {
        let app = TestApp::new().await;
        let mut req = TestRequest::new(Header::default());
        let resp = msg().handle(&app, &mut req).await.unwrap();
        assert_eq!(resp.ack_id, b"ack");

        let app = TestApp::non_legacy().await;
        msg().handle(&app, &mut req).await.unwrap_err();
    }
}
