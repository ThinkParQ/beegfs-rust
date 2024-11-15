use super::*;
use shared::bee_msg::buddy_group::SetMirrorBuddyGroup;
use shared::bee_msg::storage_pool::RefreshStoragePools;

/// Creates a new buddy group
pub(crate) async fn create_buddy_group(
    app: &impl App,
    req: pm::CreateBuddyGroupRequest,
) -> Result<pm::CreateBuddyGroupResponse> {
    fail_on_missing_license(app, LicensedFeature::Mirroring)?;
    fail_on_pre_shutdown(app)?;

    let node_type: NodeTypeServer = req.node_type().try_into()?;
    let alias: Alias = required_field(req.alias)?.try_into()?;
    let num_id: BuddyGroupId = req.num_id.unwrap_or_default().try_into()?;
    let p_target: EntityId = required_field(req.primary_target)?.try_into()?;
    let s_target: EntityId = required_field(req.secondary_target)?.try_into()?;

    let (group, p_target, s_target) = app
        .write_tx(move |tx| {
            let p_target = p_target.resolve(tx, EntityType::Target)?;
            let s_target = s_target.resolve(tx, EntityType::Target)?;

            let (group_uid, group_id) = db::buddy_group::insert(
                tx,
                num_id,
                Some(alias.clone()),
                node_type,
                p_target.num_id().try_into()?,
                s_target.num_id().try_into()?,
            )?;
            Ok((
                EntityIdSet {
                    uid: group_uid,
                    alias,
                    legacy_id: LegacyId {
                        node_type: node_type.into(),
                        num_id: group_id.into(),
                    },
                },
                p_target,
                s_target,
            ))
        })
        .await?;

    log::info!("Buddy group created: {group}");

    app.send_notifications(
        &[NodeType::Meta, NodeType::Storage, NodeType::Client],
        &SetMirrorBuddyGroup {
            ack_id: "".into(),
            node_type: node_type.into(),
            primary_target_id: p_target.num_id().try_into()?,
            secondary_target_id: s_target.num_id().try_into()?,
            group_id: group.num_id().try_into()?,
            allow_update: 0,
        },
    )
    .await;

    // Storage buddy groups alter pool membership, so trigger an immediate pool refresh
    if node_type == NodeTypeServer::Storage {
        app.send_notifications(
            &[NodeType::Meta, NodeType::Storage],
            &RefreshStoragePools { ack_id: "".into() },
        )
        .await;
    }

    Ok(pm::CreateBuddyGroupResponse {
        group: Some(group.into()),
    })
}
