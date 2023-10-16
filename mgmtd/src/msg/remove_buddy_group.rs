use super::*;
use anyhow::bail;
use shared::msg::remove_buddy_group::{RemoveBuddyGroup, RemoveBuddyGroupResp};
use shared::types::NodeTypeServer;

pub(super) async fn handle(
    msg: RemoveBuddyGroup,
    ctx: &Context,
    _req: &impl Request,
) -> RemoveBuddyGroupResp {
    match async {
        if msg.node_type != NodeTypeServer::Storage {
            bail!("Can only remove storage buddy groups");
        }

        let node_ids = ctx
            .db
            .op(move |tx| {
                db::buddy_group::validate_ids(tx, &[msg.buddy_group_id], NodeTypeServer::Storage)?;

                db::buddy_group::prepare_storage_deletion(tx, msg.buddy_group_id)
            })
            .await?;

        let res_primary: RemoveBuddyGroupResp = ctx.conn.request(node_ids.0, &msg).await?;
        let res_secondary: RemoveBuddyGroupResp = ctx.conn.request(node_ids.1, &msg).await?;

        if res_primary.result != OpsErr::SUCCESS || res_secondary.result != OpsErr::SUCCESS {
            bail!(
                "Removing storage buddy group on primary and/or secondary storage node failed.
                Primary result: {:?}, Secondary result: {:?}",
                res_primary.result,
                res_secondary.result
            );
        }

        ctx.db
            .op(move |tx| db::buddy_group::delete_storage(tx, msg.buddy_group_id))
            .await?;

        Ok(())
    }
    .await
    {
        Ok(_) => RemoveBuddyGroupResp {
            result: OpsErr::SUCCESS,
        },
        Err(err) => {
            log_error_chain!(
                err,
                "Removing {:?} buddy group {} failed",
                msg.node_type,
                msg.buddy_group_id
            );

            RemoveBuddyGroupResp {
                result: match err.downcast_ref::<TypedError>() {
                    Some(TypedError::ValueNotFound { .. }) => OpsErr::UNKNOWN_TARGET,
                    Some(_) | None => OpsErr::INTERNAL,
                },
            }
        }
    }
}
