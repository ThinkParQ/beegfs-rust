use super::*;
use db::misc::MetaRoot;

pub(super) async fn handle(
    msg: msg::SetMetadataMirroring,
    ci: impl ComponentInteractor,
    _rcc: &impl RequestConnectionController,
) -> msg::SetMetadataMirroringResp {
    match async {
        match ci.execute_db(db::misc::get_meta_root).await? {
            MetaRoot::Normal(_, node_uid) => {
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
