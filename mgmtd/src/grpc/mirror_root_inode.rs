use super::*;
use db::misc::MetaRoot;
use shared::bee_msg::OpsErr;
use shared::bee_msg::buddy_group::{SetMetadataMirroring, SetMetadataMirroringResp};

/// Enable metadata mirroring for the root directory
pub(crate) async fn mirror_root_inode(
    app: &impl AppExt,
    _req: pm::MirrorRootInodeRequest,
) -> Result<pm::MirrorRootInodeResponse> {
    app.fail_on_missing_license(LicensedFeature::Mirroring)?;
    app.fail_on_pre_shutdown()?;

    let meta_root = app
        .read_tx(|tx| {
            let node_uid = match db::misc::get_meta_root(tx)? {
                MetaRoot::Normal(_, node_uid) => node_uid,
                MetaRoot::Mirrored(_) => bail!("Root inode is already mirrored"),
                MetaRoot::Unknown => bail!("Root inode unknown"),
            };

            let count = tx.query_row(
                sql!(
                    "SELECT COUNT(*) FROM root_inode AS ri
                    INNER JOIN buddy_groups AS mg
                        ON mg.p_target_id = ri.target_id AND mg.node_type = ?1"
                ),
                [NodeType::Meta.sql_variant()],
                |row| row.get::<_, i64>(0),
            )?;

            if count < 1 {
                bail!("The meta target holding the root inode is not part of a buddy group.");
            }

            // Check that no clients are connected to prevent data corruption. Note that there is
            // still a small chance for a client being mounted again before the action is taken on
            // the root meta server below. In the end, it's the administrators responsibility to not
            // let any client mount during that process.
            let clients = tx.query_row(sql!("SELECT COUNT(*) FROM client_nodes"), [], |row| {
                row.get::<_, i64>(0)
            })?;

            if clients > 0 {
                bail!("This operation requires that all clients are disconnected/unmounted, but still has {clients} clients mounted.");
            }

            Ok(node_uid)
        })
        .await?;

    let resp: SetMetadataMirroringResp = app.request(meta_root, &SetMetadataMirroring {}).await?;

    match resp.result {
        OpsErr::SUCCESS => app.write_tx(db::misc::enable_metadata_mirroring).await?,
        _ => bail!(
            "The root meta server failed to mirror the root inode: {:?}",
            resp.result
        ),
    }

    log::info!("Root inode has been mirrored");
    Ok(pm::MirrorRootInodeResponse {})
}
