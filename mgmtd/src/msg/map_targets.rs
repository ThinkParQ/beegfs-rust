use super::*;

pub(super) async fn handle(
    msg: msg::MapTargets,
    ctx: &impl AppContext,
    _req: &impl Request,
) -> msg::MapTargetsResp {
    let target_ids = msg.target_ids.keys().copied().collect::<Vec<_>>();

    match ctx
        .db_op(move |tx| {
            // Check node ID exists
            if db::node::get_uid(tx, msg.node_id, NodeType::Storage)?.is_none() {
                return Err(DbError::value_not_found("node ID", msg.node_id));
            }

            // Check all target IDs exist
            db::target::validate_ids(tx, &target_ids, NodeTypeServer::Storage)?;

            let updated = db::target::update_storage_node_mappings(tx, &target_ids, msg.node_id)?;

            Ok(updated)
        })
        .await
    {
        Ok(updated) => {
            log::info!("Mapped {} storage targets to node {}", updated, msg.node_id);

            // TODO only do it with successful ones
            ctx.notify_nodes(
                &[NodeType::Meta, NodeType::Storage, NodeType::Client],
                &msg::MapTargets {
                    target_ids: msg.target_ids.clone(),
                    node_id: msg.node_id,
                    ack_id: "".into(),
                },
            )
            .await;

            // Storage server expects a separate status code for each target map requested.
            // For simplicity we just do an all-or-nothing approach: If all mappings succeed, we
            // return success. If at least one fails, we fail the whole operation and send back an
            // empty result (see below). The storage handles this as errors. This mechanism is
            // supposed to go away later anyway, so this solution is fine.
            msg::MapTargetsResp {
                results: msg
                    .target_ids
                    .into_iter()
                    .map(|e| (e.0, OpsErr::SUCCESS))
                    .collect(),
            }
        }
        Err(err) => {
            log_error_chain!(err, "Mapping storage targets failed");

            msg::MapTargetsResp {
                results: HashMap::new(),
            }
        }
    }
}
