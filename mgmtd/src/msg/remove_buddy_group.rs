use super::*;
use anyhow::bail;
use shared::msg::RemoveBuddyGroupResp;

pub(super) async fn handle(
    msg: msg::RemoveBuddyGroup,
    rcc: impl RequestConnectionController,
    ci: impl ComponentInteractor,
) -> Result<()> {
    match async {
        if msg.node_type != NodeTypeServer::Storage {
            bail!("Can only remove storage buddy groups");
        }

        let node_ids = ci
            .execute_db(move |tx| db::buddy_groups::prepare_storage_deletion(tx, msg.group_id))
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

        ci.execute_db(move |tx| db::buddy_groups::delete_storage(tx, msg.group_id))
            .await?;

        Ok(()) as Result<()>
    }
    .await
    {
        Ok(_) => {
            rcc.respond(&msg::RemoveBuddyGroupResp {
                result: OpsErr::SUCCESS,
            })
            .await
        }
        Err(err) => {
            log::error!(
                "Removing {} buddy group {} failed:\n{:?}",
                msg.node_type,
                msg.group_id,
                err
            );

            rcc.respond(&msg::RemoveBuddyGroupResp {
                result: OpsErr::INTERNAL,
            })
            .await
        }
    }
}
