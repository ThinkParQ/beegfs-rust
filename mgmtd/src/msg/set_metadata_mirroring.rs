use super::*;
use anyhow::bail;
use db::misc::MetaRoot;

pub(super) async fn handle(
    msg: msg::SetMetadataMirroring,
    rcc: impl RequestConnectionController,
    ci: impl ComponentInteractor,
) -> Result<()> {
    match async {
        match ci.execute_db(db::misc::get_meta_root).await? {
            MetaRoot::Normal(_, _, node_uid) => {
                let _: msg::SetMetadataMirroringResp =
                    ci.request(PeerID::Node(node_uid), &msg).await?;

                ci.execute_db(db::misc::enable_metadata_mirroring).await?;
            }
            MetaRoot::Unknown => bail!("No root inode defined"),
            MetaRoot::Mirrored(_) => bail!("Root inode is already mirrored"),
        }

        Ok(()) as Result<()>
    }
    .await
    {
        Ok(_) => {
            log::info!("Enabled metadata mirroring");

            rcc.respond(&msg::SetMetadataMirroringResp {
                result: OpsErr::SUCCESS,
            })
            .await
        }
        Err(err) => {
            log::error!("Enabling metadata mirroring failed:\n{:?}", err);

            rcc.respond(&msg::SetMetadataMirroringResp {
                result: OpsErr::INTERNAL,
            })
            .await
        }
    }
}
