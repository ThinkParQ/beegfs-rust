use super::*;
use db::target::TargetCapacities;
use shared::bee_msg::target::*;

impl HandleWithResponse for SetStorageTargetInfo {
    type Response = SetStorageTargetInfoResp;

    fn error_response() -> Self::Response {
        SetStorageTargetInfoResp {
            result: OpsErr::INTERNAL,
        }
    }

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(ctx)?;

        let node_type = self.node_type;
        ctx.db
            .write_tx(move |tx| {
                db::target::get_and_update_capacities(
                    tx,
                    self.info.into_iter().map(|e| {
                        Ok((
                            e.target_id,
                            TargetCapacities {
                                total_space: Some(e.total_space.try_into()?),
                                total_inodes: Some(e.total_inodes.try_into()?),
                                free_space: Some(e.free_space.try_into()?),
                                free_inodes: Some(e.free_inodes.try_into()?),
                            },
                        ))
                    }),
                    self.node_type.try_into()?,
                )
            })
            .await?;

        log::debug!("Updated {node_type:?} target info");

        // in the old mgmtd, a notice to refresh cap pools is sent out here if a cap pool
        // changed I consider this being to expensive to check here and just don't
        // do it. Nodes refresh their cap pool every two minutes (by default) anyway

        Ok(SetStorageTargetInfoResp {
            result: OpsErr::SUCCESS,
        })
    }
}
