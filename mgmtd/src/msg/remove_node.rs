use super::*;
use crate::types::NodeType;
use shared::msg::remove_node::{RemoveNode, RemoveNodeResp};

pub(super) async fn handle(msg: RemoveNode, ctx: &Context, _req: &impl Request) -> RemoveNodeResp {
    match ctx
        .db
        .op(move |tx| {
            let node_uid = db::node::get_uid(tx, msg.node_id, msg.node_type.try_into()?)?
                .ok_or_else(|| TypedError::value_not_found("node ID", msg.node_id))?;

            db::node::delete(tx, node_uid)?;

            Ok(())
        })
        .await
    {
        Ok(_) => {
            log::info!("Removed {:?} node with ID {}", msg.node_type, msg.node_id,);

            notify_nodes(
                ctx,
                match msg.node_type {
                    shared::types::NodeType::Meta => &[NodeType::Meta, NodeType::Client],
                    shared::types::NodeType::Storage => {
                        &[NodeType::Meta, NodeType::Storage, NodeType::Client]
                    }
                    _ => &[],
                },
                &RemoveNode {
                    ack_id: "".into(),
                    ..msg
                },
            )
            .await;

            RemoveNodeResp {
                result: OpsErr::SUCCESS,
            }
        }
        Err(err) => {
            log_error_chain!(
                err,
                "Removing {:?} node with ID {} failed",
                msg.node_type,
                msg.node_id
            );

            RemoveNodeResp {
                result: OpsErr::INTERNAL,
            }
        }
    }
}
