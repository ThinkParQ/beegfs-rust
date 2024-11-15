use super::*;
use shared::bee_msg::OpsErr;
use shared::bee_msg::buddy_group::{RemoveBuddyGroup, RemoveBuddyGroupResp};
use shared::bee_msg::storage_pool::RefreshStoragePools;

/// Deletes a buddy group. This function is racy as it is a two step process, talking to other
/// nodes in between. Since it is rarely used, that's ok though.
pub(crate) async fn delete_buddy_group(
    app: &impl App,
    req: pm::DeleteBuddyGroupRequest,
) -> Result<pm::DeleteBuddyGroupResponse> {
    fail_on_missing_license(app, LicensedFeature::Mirroring)?;
    fail_on_pre_shutdown(app)?;

    let group: EntityId = required_field(req.group)?.try_into()?;
    let execute: bool = required_field(req.execute)?;

    // 1. Check deletion is allowed
    let (group, p_node_uid, s_node_uid) = app
        .db_conn(move |conn| {
            let tx = conn.transaction()?;

            let group = group.resolve(&tx, EntityType::BuddyGroup)?;

            if group.node_type() != NodeType::Storage {
                bail!("Only storage buddy groups can be deleted");
            }

            let (p_node_uid, s_node_uid) =
                db::buddy_group::prepare_storage_deletion(&tx, group.num_id().try_into()?)?;

            if execute {
                tx.commit()?;
            }
            Ok((group, p_node_uid, s_node_uid))
        })
        .await?;

    // 2. Forward request to the groups nodes
    let group_id: BuddyGroupId = group.num_id().try_into()?;
    let remove_bee_msg = RemoveBuddyGroup {
        node_type: NodeType::Storage,
        group_id,
        check_only: if execute { 0 } else { 1 },
        force: 0,
    };

    let p_res: RemoveBuddyGroupResp = app.request(p_node_uid, &remove_bee_msg).await?;
    let s_res: RemoveBuddyGroupResp = app.request(s_node_uid, &remove_bee_msg).await?;

    if p_res.result != OpsErr::SUCCESS || s_res.result != OpsErr::SUCCESS {
        bail!(
            "Removing storage buddy group on primary and/or secondary storage node failed. \
Primary result: {:?}, Secondary result: {:?}",
            p_res.result,
            s_res.result
        );
    }

    // 3. If the deletion request succeeded, remove the group from the database
    app.db_conn(move |conn| {
        let tx = conn.transaction()?;

        db::buddy_group::delete_storage(&tx, group_id)?;

        if execute {
            tx.commit()?;
        }
        Ok(())
    })
    .await?;

    if execute {
        log::info!("Buddy group deleted: {group}");

        // Storage buddy groups alter pool membership, so trigger an immediate pool refresh
        app.send_notifications(
            &[NodeType::Meta, NodeType::Storage],
            &RefreshStoragePools { ack_id: "".into() },
        )
        .await;
    }

    Ok(pm::DeleteBuddyGroupResponse {
        group: Some(group.into()),
    })
}
