use super::*;
use shared::bee_msg::OpsErr;
use shared::bee_msg::target::{
    RefreshTargetStates, SetTargetConsistencyStates, SetTargetConsistencyStatesResp,
};

/// Set consistency state for a target
pub(crate) async fn set_target_state(
    ctx: Context,
    req: pm::SetTargetStateRequest,
) -> Result<pm::SetTargetStateResponse> {
    fail_on_pre_shutdown(&ctx)?;

    let state: TargetConsistencyState = req.consistency_state().try_into()?;
    let target: EntityId = required_field(req.target)?.try_into()?;

    let (target, node_uid) = ctx
        .db
        .write_tx(move |tx| {
            let target = target.resolve(tx, EntityType::Target)?;

            let node: i64 = tx.query_row_cached(
                sql!("SELECT node_uid FROM targets_ext WHERE target_uid = ?1"),
                [target.uid],
                |row| row.get(0),
            )?;

            db::target::update_consistency_states(
                tx,
                [(target.num_id().try_into()?, state)],
                NodeTypeServer::try_from(target.node_type())?,
            )?;

            Ok((target, node))
        })
        .await?;

    let resp: SetTargetConsistencyStatesResp = ctx
        .conn
        .request(
            node_uid,
            &SetTargetConsistencyStates {
                node_type: target.node_type(),
                target_ids: vec![target.num_id().try_into().unwrap()],
                states: vec![state],
                ack_id: "".into(),
                set_online: 0,
            },
        )
        .await?;
    if resp.result != OpsErr::SUCCESS {
        bail!(
            "Management successfully set the target state, but the target {target} failed to process it: {:?}",
            resp.result
        );
    }

    notify_nodes(
        &ctx,
        &[NodeType::Meta, NodeType::Storage, NodeType::Client],
        &RefreshTargetStates { ack_id: "".into() },
    )
    .await;

    Ok(pm::SetTargetStateResponse {})
}
