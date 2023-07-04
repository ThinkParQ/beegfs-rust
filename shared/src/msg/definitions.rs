use super::*;
use anyhow::Result;

macro_rules! msg_modules {
    ($($mod:ident,)*) => {
        $(
            mod $mod;
            pub use $mod::*;
        )*
    };
}

msg_modules! {
    ack,
    add_storage_pool,
    authenticate_channel,
    change_target_consistency_states,
    generic_response,
    get_default_quota,
    get_mirror_buddy_groups,
    get_node_capacity_pools,
    get_nodes,
    get_quota_info,
    get_states_and_buddy_groups,
    get_storage_pools,
    get_target_mappings,
    get_target_states,
    heartbeat,
    map_targets,
    modify_storage_pool,
    peer_info,
    publish_capacities,
    refresh_capacity_pools,
    refresh_storage_pools,
    refresh_target_states,
    register_node,
    register_storage_target,
    remove_buddy_group,
    remove_node,
    remove_storage_pool,
    request_exceeded_quota,
    set_channel_direct,
    set_default_quota,
    set_exceeded_quota,
    set_metadata_mirroring,
    set_mirror_buddy_group,
    set_quota,
    set_target_consistency_states,
    set_target_info,
    unmap_storage_target,
}
