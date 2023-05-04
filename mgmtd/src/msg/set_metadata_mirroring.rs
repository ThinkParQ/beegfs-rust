use super::*;
use anyhow::bail;
use db::misc::MetaRoot;

pub(super) async fn handle(
    msg: msg::SetMetadataMirroring,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    match async {
        match hnd.execute_db(db::misc::get_meta_root).await? {
            MetaRoot::Normal(target_uid, _, node_uid) => {
                let _: msg::SetMetadataMirroringResp =
                    hnd.request(PeerID::Node(node_uid), &msg).await?;

                hnd.execute_db(move |tx| db::misc::enable_metadata_mirroring(tx, target_uid))
                    .await?;
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

            chn.respond(&msg::SetMetadataMirroringResp {
                result: OpsErr::SUCCESS,
            })
            .await
        }
        Err(err) => {
            log::error!("Enabling metadata mirroring failed:\n{:?}", err);

            chn.respond(&msg::SetMetadataMirroringResp {
                result: OpsErr::INTERNAL,
            })
            .await
        }
    }
}
