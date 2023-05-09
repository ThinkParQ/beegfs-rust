use shared::TargetReachabilityState;
use std::time::Duration;

pub(crate) fn calc_reachability_state(age: Duration, timeout: Duration) -> TargetReachabilityState {
    if age < timeout {
        TargetReachabilityState::Online
    } else if age < timeout / 2 {
        TargetReachabilityState::ProbablyOffline
    } else {
        TargetReachabilityState::Offline
    }
}
