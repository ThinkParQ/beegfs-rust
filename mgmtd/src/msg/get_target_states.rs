use super::*;

pub(super) async fn handle(
    msg: msg::GetTargetStates,
    ctx: &impl AppContext,
    _req: &impl Request,
) -> msg::GetTargetStatesResp {
    match ctx
        .db_op(move |tx| db::target::get_with_type(tx, msg.node_type))
        .await
    {
        Ok(res) => {
            let mut targets = Vec::with_capacity(res.len());
            let mut reachability_states = Vec::with_capacity(res.len());
            let mut consistency_states = Vec::with_capacity(res.len());

            for e in res {
                targets.push(e.target_id);
                reachability_states.push(db::misc::calc_reachability_state(
                    e.last_contact,
                    ctx.get_config().node_offline_timeout,
                ));
                consistency_states.push(e.consistency);
            }

            msg::GetTargetStatesResp {
                targets,
                reachability_states,
                consistency_states,
            }
        }
        Err(err) => {
            log_error_chain!(
                err,
                "Getting target states for {} nodes failed",
                msg.node_type,
            );

            msg::GetTargetStatesResp {
                targets: vec![],
                reachability_states: vec![],
                consistency_states: vec![],
            }
        }
    }
}
