use super::*;
use crate::db::targets::TargetCapacities;
use crate::db::NonexistingKey;

pub(super) async fn handle(
    msg: msg::SetTargetInfo,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    let node_type = msg.node_type;
    match hnd
        .execute_db(move |tx| {
            db::targets::get_and_update_capacities(
                tx,
                msg.info.into_iter().map(|e| {
                    (
                        e.target_id,
                        TargetCapacities {
                            total_space: Some(e.total_space),
                            total_inodes: Some(e.total_inodes),
                            free_space: Some(e.free_space),
                            free_inodes: Some(e.free_inodes),
                        },
                    )
                }),
                msg.node_type,
            )
        })
        .await
    {
        Ok(_) => {
            log::info!("Updated {} target info", node_type,);

            // in the old mgmtd, a notice to refresh cap pools is sent out here if a cap pool
            // changed I consider this being to expensive to check here and just don't
            // do it. Nodes refresh their cap pool every two minutes (by default) anyway

            chn.respond(&msg::SetTargetInfoResp {
                result: OpsErr::SUCCESS,
            })
            .await
        }

        Err(err) => {
            log::error!("Updating {} target info failed:\n{:?}", node_type, err);
            chn.respond(&msg::SetTargetInfoResp {
                result: match err.downcast_ref() {
                    Some(NonexistingKey(_)) => OpsErr::INVAL,
                    None => OpsErr::INTERNAL,
                },
            })
            .await
        }
    }
}
