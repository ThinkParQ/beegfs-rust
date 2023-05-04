use shared::{CapPoolLimits, CapacityPool, TargetReachabilityState};
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

pub(crate) fn calc_cap_pool(
    limits: &CapPoolLimits,
    free_space: Option<u64>,
    free_inodes: Option<u64>,
) -> CapacityPool {
    if let Some(free_space) = free_space {
        if let Some(free_inodes) = free_inodes {
            if free_space < limits.space_emergency || free_inodes < limits.inode_emergency {
                CapacityPool::Emergency
            } else if free_space < limits.space_low || free_inodes < limits.inode_low {
                CapacityPool::Low
            } else {
                CapacityPool::Normal
            }
        } else {
            CapacityPool::Emergency
        }
    } else {
        CapacityPool::Emergency
    }
}
