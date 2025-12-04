use super::*;
use assign_pool::do_assign;
use shared::bee_msg::storage_pool::RefreshStoragePools;

/// Creates a new pool, optionally assigning targets and groups
pub(crate) async fn create_pool(
    app: &impl App,
    req: pm::CreatePoolRequest,
) -> Result<pm::CreatePoolResponse> {
    fail_on_missing_license(app, LicensedFeature::Storagepool)?;
    fail_on_pre_shutdown(app)?;

    if req.node_type() != pb::NodeType::Storage {
        bail!("node type must be storage");
    }

    let alias: Alias = required_field(req.alias)?.try_into()?;
    let num_id: PoolId = req.num_id.unwrap_or_default().try_into()?;

    let (pool_uid, alias, pool_id) = app
        .write_tx(move |tx| {
            let (pool_uid, pool_id) = db::storage_pool::insert(tx, num_id, &alias)?;
            do_assign(tx, pool_id, req.targets, req.buddy_groups)?;
            Ok((pool_uid, alias, pool_id))
        })
        .await?;

    let pool = EntityIdSet {
        uid: pool_uid,
        alias,
        legacy_id: LegacyId {
            node_type: NodeType::Storage,
            num_id: pool_id.into(),
        },
    };

    log::info!("Pool created: {pool}");

    app.send_notifications(
        &[NodeType::Meta, NodeType::Storage],
        &RefreshStoragePools { ack_id: "".into() },
    )
    .await;

    Ok(pm::CreatePoolResponse {
        pool: Some(pool.into()),
    })
}
