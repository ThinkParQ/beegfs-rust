use super::*;
use shared::msg::get_target_states::{
    GetTargetStates, GetTargetStatesResp, TargetReachabilityState,
};
use std::time::Duration;

pub(super) async fn handle(
    msg: GetTargetStates,
    ctx: &Context,
    _req: &impl Request,
) -> GetTargetStatesResp {
    match ctx
        .db
        .op(move |tx| db::target::get_with_type(tx, msg.node_type.try_into()?))
        .await
    {
        Ok(res) => {
            let mut targets = Vec::with_capacity(res.len());
            let mut reachability_states = Vec::with_capacity(res.len());
            let mut consistency_states = Vec::with_capacity(res.len());

            for e in res {
                targets.push(e.target_id);
                reachability_states.push(calc_reachability_state(
                    e.last_contact,
                    ctx.info.user_config.node_offline_timeout,
                ));
                consistency_states.push(e.consistency.into());
            }

            GetTargetStatesResp {
                targets,
                reachability_states,
                consistency_states,
            }
        }
        Err(err) => {
            log_error_chain!(
                err,
                "Getting target states for {:?} nodes failed",
                msg.node_type,
            );

            GetTargetStatesResp {
                targets: vec![],
                reachability_states: vec![],
                consistency_states: vec![],
            }
        }
    }
}

/// Calculate reachability state as requested by old BeeGFS code.
pub fn calc_reachability_state(
    contact_age: Duration,
    timeout: Duration,
) -> TargetReachabilityState {
    if contact_age < timeout {
        TargetReachabilityState::Online
    } else if contact_age < timeout / 2 {
        TargetReachabilityState::ProbablyOffline
    } else {
        TargetReachabilityState::Offline
    }
}
