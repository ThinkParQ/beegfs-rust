use super::*;
use shared::bee_msg::storage_pool::RefreshStoragePools;
use shared::bee_msg::target::*;

impl HandleWithResponse for MapTargets {
    type Response = MapTargetsResp;

    async fn handle(self, app: &impl App, _req: &mut impl Request) -> Result<Self::Response> {
        fail_on_pre_shutdown(app)?;

        let target_ids = self.target_ids.keys().copied().collect::<Vec<_>>();

        let updated = app
            .write_tx(move |tx| {
                // Check node Id exists
                let node = LegacyId {
                    node_type: NodeType::Storage,
                    num_id: self.node_id,
                }
                .resolve(tx, EntityType::Node)?;
                // Check all target Ids exist
                db::target::validate_ids(tx, &target_ids, NodeTypeServer::Storage)?;
                // Due to the check above, this must always match all the given ids
                let updated =
                    db::target::update_storage_node_mappings(tx, &target_ids, node.num_id())?;
                Ok(updated)
            })
            .await?;

        // At this point, all mappings must have been successful

        log::info!(
            "Mapped storage targets with Ids {:?} to node {}",
            self.target_ids.keys(),
            self.node_id
        );

        app.send_notifications(
            &[NodeType::Meta, NodeType::Storage, NodeType::Client],
            &MapTargets {
                target_ids: self.target_ids.clone(),
                node_id: self.node_id,
                ack_id: "".into(),
            },
        )
        .await;

        // Map targets alter pool membership, so trigger an immediate pool refresh
        if updated > 0 {
            app.send_notifications(
                &[NodeType::Meta, NodeType::Storage],
                &RefreshStoragePools { ack_id: "".into() },
            )
            .await;
        }

        // Storage server expects a separate status code for each target map requested. We, however,
        // do a all-or-nothing approach. If e.g. one target id doesn't exist (which is an
        // exceptional error and should usually not happen anyway), we fail the whole
        // operation. Therefore we can just send a list of successes.
        let resp = MapTargetsResp {
            results: self
                .target_ids
                .into_iter()
                .map(|e| (e.0, OpsErr::SUCCESS))
                .collect(),
        };

        Ok(resp)
    }
}
