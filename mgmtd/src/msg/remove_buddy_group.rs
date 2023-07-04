use super::*;
use anyhow::bail;
use shared::msg::RemoveBuddyGroupResp;

pub(super) async fn handle(
    msg: msg::RemoveBuddyGroup,
    ci: impl ComponentInteractor,
    _rcc: &impl RequestConnectionController,
) -> msg::RemoveBuddyGroupResp {
    match async {
        if msg.node_type != NodeTypeServer::Storage {
            bail!("Can only remove storage buddy groups");
        }

        let node_ids = ci
            .db_op(move |tx| {
                db::buddy_group::check_existence(
                    tx,
                    &[msg.buddy_group_id],
                    NodeTypeServer::Storage,
                )?;

                db::buddy_group::prepare_storage_deletion(tx, msg.buddy_group_id)
            })
            .await?;

        let res_primary: RemoveBuddyGroupResp = ci.request(PeerID::Node(node_ids.0), &msg).await?;
        let res_secondary: RemoveBuddyGroupResp =
            ci.request(PeerID::Node(node_ids.1), &msg).await?;

        if res_primary.result != OpsErr::SUCCESS || res_secondary.result != OpsErr::SUCCESS {
            bail!(
                "Removing storage buddy group on primary and/or secondary storage node failed.
                Primary result: {:?}, Secondary result: {:?}",
                res_primary.result,
                res_secondary.result
            );
        }

        ci.db_op(move |tx| db::buddy_group::delete_storage(tx, msg.buddy_group_id))
            .await?;

        Ok(())
    }
    .await
    {
        Ok(_) => msg::RemoveBuddyGroupResp {
            result: OpsErr::SUCCESS,
        },
        Err(err) => {
            log_error_chain!(
                err,
                "Removing {} buddy group {} failed",
                msg.node_type,
                msg.buddy_group_id
            );

            msg::RemoveBuddyGroupResp {
                result: match err.downcast_ref::<DbError>() {
                    Some(DbError::ValueNotFound { .. }) => OpsErr::UNKNOWN_TARGET,
                    Some(_) | None => OpsErr::INTERNAL,
                },
            }
        }
    }
}
