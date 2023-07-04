use super::*;
use crate::db::target::TargetCapacities;

pub(super) async fn handle(
    msg: msg::SetTargetInfo,
    ci: impl ComponentInteractor,
    _rcc: &impl RequestConnectionController,
) -> msg::SetTargetInfoResp {
    let node_type = msg.node_type;
    match ci
        .db_op(move |tx| {
            db::target::get_and_update_capacities(
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

            msg::SetTargetInfoResp {
                result: OpsErr::SUCCESS,
            }
        }

        Err(err) => {
            log_error_chain!(err, "Updating {} target info failed", node_type);
            msg::SetTargetInfoResp {
                result: OpsErr::INTERNAL,
            }
        }
    }
}
