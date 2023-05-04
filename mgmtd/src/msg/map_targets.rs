use super::*;

pub(super) async fn handle(
    msg: msg::MapTargets,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    let mut results = HashMap::with_capacity(msg.targets.len());

    for (target_id, _) in msg.targets.clone() {
        let res = hnd
            .execute_db(move |tx| {
                db::targets::update_node(tx, target_id, NodeTypeServer::Storage, msg.node_num_id)
            })
            .await;

        results.insert(
            target_id,
            match res {
                Ok(_) => OpsErr::SUCCESS,
                Err(err) => {
                    log::error!(
                        "Mapping storage target {} to node {} failed:\n{:?}",
                        target_id,
                        msg.node_num_id,
                        err
                    );
                    // TODO correct result code
                    OpsErr::INTERNAL
                }
            },
        );
    }

    let fails = results.iter().filter(|e| *e.1 == OpsErr::INTERNAL).count();

    if fails == 0 {
        log::info!(
            "Mapped {} storage targets to node {}",
            results.len(),
            msg.node_num_id
        );
    } else {
        log::info!(
            "Tried to map {} storage targets to node {}, but {} failures occurred",
            results.len(),
            msg.node_num_id,
            fails
        );
    };

    // forward to all nodes if
    // TODO only do it with successful ones
    hnd.notify_nodes(&msg::MapTargets {
        targets: msg.targets,
        node_num_id: msg.node_num_id,
        ack_id: "".into(),
    })
    .await;

    chn.respond(&msg::MapTargetsResp { results }).await
}
