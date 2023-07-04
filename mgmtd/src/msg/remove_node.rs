use super::*;

pub(super) async fn handle(
    msg: msg::RemoveNode,
    ci: impl ComponentInteractor,
    _rcc: &impl RequestConnectionController,
) -> msg::RemoveNodeResp {
    match ci
        .db_op(move |tx| {
            let node_uid = db::node::get_uid(tx, msg.node_id, msg.node_type)?
                .ok_or_else(|| DbError::value_not_found("node ID", msg.node_id))?;

            db::node::delete(tx, node_uid)?;

            Ok(())
        })
        .await
    {
        Ok(_) => {
            log::info!("Removed {} node with ID {}", msg.node_type, msg.node_id,);

            ci.notify_nodes(&msg::RemoveNode {
                ack_id: "".into(),
                ..msg
            })
            .await;

            msg::RemoveNodeResp {
                result: OpsErr::SUCCESS,
            }
        }
        Err(err) => {
            log_error_chain!(
                err,
                "Removing {} node with ID {} failed",
                msg.node_type,
                msg.node_id
            );

            msg::RemoveNodeResp {
                result: OpsErr::INTERNAL,
            }
        }
    }
}
