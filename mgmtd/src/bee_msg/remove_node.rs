use super::*;
use shared::bee_msg::node::*;

impl HandleWithResponse for RemoveNode {
    type Response = RemoveNodeResp;

    fn error_response() -> Self::Response {
        RemoveNodeResp {
            result: OpsErr::INTERNAL,
        }
    }

    async fn handle(self, app: &impl App, _req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(app)?;

        let node = app
            .write_tx(move |tx| {
                if self.node_type != NodeType::Client {
                    bail!(
                        "This BeeMsg handler can only delete client nodes. \
For server nodes, the grpc handler must be used."
                    );
                }

                let node = LegacyId {
                    node_type: self.node_type,
                    num_id: self.node_id,
                }
                .resolve(tx, EntityType::Node)?;

                db::node::delete(tx, node.uid)?;

                Ok(node)
            })
            .await?;

        log::info!("Node deleted: {node}");

        app.send_notifications(
            match self.node_type {
                shared::types::NodeType::Meta => &[NodeType::Meta, NodeType::Client],
                shared::types::NodeType::Storage => {
                    &[NodeType::Meta, NodeType::Storage, NodeType::Client]
                }
                _ => &[],
            },
            &RemoveNode {
                ack_id: "".into(),
                ..self
            },
        )
        .await;

        Ok(RemoveNodeResp {
            result: OpsErr::SUCCESS,
        })
    }
}
