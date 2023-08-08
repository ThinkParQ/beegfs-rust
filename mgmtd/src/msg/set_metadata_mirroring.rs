use super::*;
use db::misc::MetaRoot;

pub(super) async fn handle(
    msg: msg::SetMetadataMirroring,
    ctx: &impl AppContext,
    _req: &impl Request,
) -> msg::SetMetadataMirroringResp {
    match async {
        match ctx.db_op(db::misc::get_meta_root).await? {
            MetaRoot::Normal(_, node_uid) => {
                let _: msg::SetMetadataMirroringResp = ctx.request(node_uid, &msg).await?;

                ctx.db_op(db::misc::enable_metadata_mirroring).await?;
            }
            MetaRoot::Unknown => bail!("Root inode unknown"),
            MetaRoot::Mirrored(_) => bail!("Root inode is already mirrored"),
        }

        Ok(()) as Result<()>
    }
    .await
    {
        Ok(_) => {
            log::info!("Enabled metadata mirroring");

            msg::SetMetadataMirroringResp {
                result: OpsErr::SUCCESS,
            }
        }
        Err(err) => {
            log_error_chain!(err, "Enabling metadata mirroring failed");

            msg::SetMetadataMirroringResp {
                result: OpsErr::INTERNAL,
            }
        }
    }
}
