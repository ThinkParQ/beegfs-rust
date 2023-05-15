use super::*;
use crate::db::NonexistingKey;

pub(super) async fn handle(
    msg: msg::RemoveNode,
    rcc: impl RequestConnectionController,
    ci: impl ComponentInteractor,
) -> Result<()> {
    match ci
        .execute_db(move |tx| db::nodes::delete(tx, msg.num_id, msg.node_type))
        .await
    {
        Ok(_) => {
            log::info!("Removed {} node with ID {}", msg.node_type, msg.num_id,);

            ci.notify_nodes(&msg::RemoveNode {
                ack_id: "".into(),
                ..msg
            })
            .await;

            rcc.respond(&msg::RemoveNodeResp {
                result: OpsErr::SUCCESS,
            })
            .await
        }
        Err(err) => {
            log::error!(
                "Removing {} node with ID {} failed:\n{:?}",
                msg.node_type,
                msg.num_id,
                err
            );

            rcc.respond(&msg::RemoveNodeResp {
                result: match err.downcast_ref() {
                    Some(NonexistingKey(_)) => OpsErr::UNKNOWN_NODE,
                    None => OpsErr::INTERNAL,
                },
            })
            .await
        }
    }
}
