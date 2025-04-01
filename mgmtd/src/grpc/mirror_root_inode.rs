use super::*;
use db::misc::MetaRoot;
use shared::bee_msg::OpsErr;
use shared::bee_msg::buddy_group::{SetMetadataMirroring, SetMetadataMirroringResp};

/// Enable metadata mirroring for the root directory
pub(crate) async fn mirror_root_inode(
    ctx: Context,
    _req: pm::MirrorRootInodeRequest,
) -> Result<pm::MirrorRootInodeResponse> {
    needs_license(&ctx, LicensedFeature::Mirroring)?;
    fail_on_pre_shutdown(&ctx)?;

    let offline_timeout = ctx.info.user_config.node_offline_timeout.as_secs();
    let meta_root = ctx
        .db
        .read_tx(move |tx| {
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
                bail!(
                    "This operation requires that all clients are disconnected/unmounted. \
{clients} clients are still mounted."
                );
            }

            let mut server_stmt = tx.prepare(sql!(
                "SELECT COUNT(*) FROM nodes
                WHERE node_type = ?1 AND UNIXEPOCH('now') - UNIXEPOCH(last_contact) < ?2
                AND node_uid != ?3"
            ))?;

            let metas = server_stmt.query_row(
                params![NodeType::Meta.sql_variant(), offline_timeout, node_uid],
                |row| row.get::<_, i64>(0),
            )?;
            let storages = server_stmt.query_row(
                params![NodeType::Storage.sql_variant(), offline_timeout, node_uid],
                |row| row.get::<_, i64>(0),
            )?;

            if metas > 0 || storages > 0 {
                bail!(
                    "This operation requires that all nodes except the root meta node are shut \
down. {metas} meta nodes (excluding the root meta node) and {storages} storage nodes have \
communicated during the last {offline_timeout}s."
                );
            }

            Ok(node_uid)
        })
        .await?;

    let resp: SetMetadataMirroringResp = ctx
        .conn
        .request(meta_root, &SetMetadataMirroring {})
        .await?;

    match resp.result {
        OpsErr::SUCCESS => ctx.db.write_tx(db::misc::enable_metadata_mirroring).await?,
        _ => bail!(
            "The root meta server failed to mirror the root inode: {:?}",
            resp.result
        ),
    }

    log::info!("Root inode has been mirrored");
    Ok(pm::MirrorRootInodeResponse {})
}
