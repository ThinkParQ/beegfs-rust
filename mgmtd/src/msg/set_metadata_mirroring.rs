use super::*;
use db::misc::MetaRoot;
use shared::msg::set_metadata_mirroring::{SetMetadataMirroring, SetMetadataMirroringResp};

pub(super) async fn handle(
    msg: SetMetadataMirroring,
    ctx: &Context,
    _req: &impl Request,
) -> SetMetadataMirroringResp {
    match async {
        match ctx.db.op(db::misc::get_meta_root).await? {
            MetaRoot::Normal(_, node_uid) => {
                let _: SetMetadataMirroringResp = ctx.conn.request(node_uid, &msg).await?;

                ctx.db.op(db::misc::enable_metadata_mirroring).await?;
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

            SetMetadataMirroringResp {
                result: OpsErr::SUCCESS,
            }
        }
        Err(err) => {
            log_error_chain!(err, "Enabling metadata mirroring failed");

            SetMetadataMirroringResp {
                result: OpsErr::INTERNAL,
            }
        }
    }
}
