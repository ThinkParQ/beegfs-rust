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
        Ok(old_capacities) => {
            log::info!("Updated {} target info", node_type,);

            // if pool allocation changed, notify nodes to refresh
            if old_capacities.iter().any(|e| {
                let cap_pool = match node_type {
                    NodeTypeServer::Meta => hnd.get_config::<config::CapPoolMetaLimits>(),
                    NodeTypeServer::Storage => hnd.get_config::<config::CapPoolStorageLimits>(),
                };
                logic::calc_cap_pool(&cap_pool, e.1.free_space, e.1.free_inodes)
                    != logic::calc_cap_pool(&cap_pool, e.1.free_space, e.1.free_inodes)
            }) {
                hnd.notify_nodes(&msg::RefreshCapacityPools { ack_id: "".into() })
                    .await;
            }

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
