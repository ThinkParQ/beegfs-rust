use super::*;
use shared::bee_msg::target::*;

impl HandleWithResponse for GetTargetMappings {
    type Response = GetTargetMappingsResp;

    async fn handle(self, app: &impl App, _req: &mut impl Request) -> Result<Self::Response> {
        let mapping: HashMap<TargetId, NodeId> = app
            .read_tx(move |tx| {
                tx.query_map_collect(
                    sql!(
                        "SELECT target_id, node_id
                        FROM storage_targets
                        WHERE node_id IS NOT NULL"
                    ),
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(Into::into)
            })
            .await?;

        let resp = GetTargetMappingsResp { mapping };

        Ok(resp)
    }
}
