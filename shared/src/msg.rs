//! BeeGFS network message definitions

use crate::bee_serde::{BeeSerde, *};
use crate::types::*;
use anyhow::Result;
use bee_serde_derive::BeeSerde;
use std::collections::{HashMap, HashSet};

pub mod ack;
pub mod add_storage_pool;
pub mod authenticate_channel;
pub mod change_target_consistency_states;
pub mod generic_response;
pub mod get_default_quota;
pub mod get_mirror_buddy_groups;
pub mod get_node_capacity_pools;
pub mod get_nodes;
pub mod get_quota_info;
pub mod get_states_and_buddy_groups;
pub mod get_storage_pools;
pub mod get_target_mappings;
pub mod get_target_states;
pub mod header;
pub mod heartbeat;
pub mod map_targets;
pub mod modify_storage_pool;
pub mod peer_info;
pub mod publish_capacities;
pub mod refresh_capacity_pools;
pub mod refresh_storage_pools;
pub mod refresh_target_states;
pub mod register_node;
pub mod register_target;
pub mod remove_buddy_group;
pub mod remove_node;
pub mod remove_storage_pool;
pub mod request_exceeded_quota;
pub mod set_channel_direct;
pub mod set_default_quota;
pub mod set_exceeded_quota;
pub mod set_metadata_mirroring;
pub mod set_mirror_buddy_group;
pub mod set_quota;
pub mod set_storage_target_info;
pub mod set_target_consistency_states;
pub mod unmap_target;

/// The BeeGFS message ID as defined in `NetMsgTypes.h`
pub type MsgID = u16;

/// A BeeGFS message
///
/// A struct that implements `Msg` represents a BeeGFS message that is compatible with other C/C++
/// based BeeGFS components.
pub trait Msg: BeeSerde + std::fmt::Debug + Clone + Send + Sync + 'static {
    /// Message type as defined in NetMessageTypes.h
    const ID: MsgID;

    /// Returns the feature flags
    ///
    /// Feature flags are a u16 field in the message header and are sometimes used to control
    /// (de-)serialization. This function provides them to the serializer.
    fn build_feature_flags(&self) -> u16 {
        0
    }
}

/// Matches the `FhgfsOpsErr` value from the BeeGFS C/C++ codebase.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct OpsErr(i32);

impl OpsErr {
    pub const SUCCESS: Self = Self(0);
    pub const INTERNAL: Self = Self(1);
    pub const UNKNOWN_NODE: Self = Self(5);
    pub const EXISTS: Self = Self(7);
    pub const NOTEMPTY: Self = Self(13);
    pub const UNKNOWN_TARGET: Self = Self(15);
    pub const INVAL: Self = Self(20);
    pub const AGAIN: Self = Self(22);
    pub const UNKNOWN_POOL: Self = Self(30);
}
