//! Various BeeGFS type definitions
//!
//! Used internally (e.g. in the management) and by network messages. Types only used by BeeGFS
//! messages are found in [crate::msg::types].

use crate::bee_serde::*;
use anyhow::Result;
use bee_serde_derive::BeeSerde;
use core::hash::Hash;
use std::fmt::Debug;

// Type aliases for convenience. Used by BeeGFS messaging and the management.
//
// CAUTION: While most known BeeGFS messages use the aliased integers below for these types, some
// do not. It still has to be checked for each BeeGFS message individually which exact type is
// needed for serialization.

pub type EntityUID = i64;
pub type TargetID = u16;
pub type BuddyGroupID = u16;
pub type Port = u16;
pub type NodeID = u16;
pub type StoragePoolID = u16;
pub type QuotaID = u32;

pub const MGMTD_ID: NodeID = 1;
pub const DEFAULT_STORAGE_POOL: StoragePoolID = 1;

/// The entity type.
#[derive(Clone, Debug)]
pub enum EntityType {
    Node,
    Target,
    BuddyGroup,
    StoragePool,
}

impl_enum_to_sql_str!(EntityType,
    Node => "node",
    Target => "target",
    BuddyGroup => "buddy_group",
    StoragePool => "storage_pool",
);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NodeType {
    #[default]
    Meta,
    Storage,
    Client,
    Management,
}

impl_enum_to_int!(NodeType,
    Meta => 1,
    Storage => 2,
    Client => 3,
    Management => 4
);
impl_enum_to_sql_str!(NodeType,
    Meta => "meta",
    Storage => "storage",
    Client => "client",
    Management => "management"
);

impl From<NodeTypeServer> for NodeType {
    fn from(value: NodeTypeServer) -> Self {
        match value {
            NodeTypeServer::Meta => Self::Meta,
            NodeTypeServer::Storage => Self::Storage,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NodeTypeServer {
    #[default]
    Meta,
    Storage,
}

impl_enum_to_int!(NodeTypeServer,
    Meta => 1,
    Storage => 2
);
impl_enum_to_sql_str!(NodeTypeServer,
    Meta => "meta",
    Storage => "storage"
);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NicType {
    #[default]
    Ethernet,
    Sdp,
    Rdma,
}

impl_enum_to_int!(NicType, Ethernet => 0, Sdp => 1, Rdma => 2);
impl_enum_to_sql_str!(NicType, Ethernet => "ethernet", Sdp => "sdp", Rdma => "rdma");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CapacityPool {
    Normal,
    Low,
    Emergency,
}

impl CapacityPool {
    pub fn lowest(cap_pool_1: Self, cap_pool_2: Self) -> Self {
        std::cmp::max(cap_pool_1, cap_pool_2)
    }
}

impl_enum_to_int!(CapacityPool, Normal => 0, Low => 1, Emergency => 2);
impl_enum_to_sql_str!(CapacityPool, Normal => "normal", Low => "low", Emergency => "emergency");

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum QuotaIDType {
    #[default]
    User,
    Group,
}

impl_enum_to_int!(QuotaIDType,
    User => 1,
    Group => 2
);
impl_enum_to_sql_str!(QuotaIDType,
    User => "user",
    Group => "group"
);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum QuotaType {
    #[default]
    Space,
    Inodes,
}

impl_enum_to_int!(QuotaType,
    Space => 1,
    Inodes => 2
);

impl_enum_to_sql_str!(QuotaType,
    Space => "space",
    Inodes => "inodes"
);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TargetConsistencyState {
    #[default]
    Good,
    NeedsResync,
    Bad,
}

impl_enum_to_int!(TargetConsistencyState,
    Good => 0,
    NeedsResync => 1,
    Bad => 2
);

impl BeeSerde for TargetConsistencyState {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.u8((*self).into())
    }

    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        des.u8()?.try_into()
    }
}

impl_enum_to_sql_str!(TargetConsistencyState,
    Good => "good",
    NeedsResync => "needs_resync",
    Bad => "bad"
);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TargetReachabilityState {
    #[default]
    Online,
    ProbablyOffline,
    Offline,
}

impl_enum_to_int!(TargetReachabilityState,
    Online => 0,
    ProbablyOffline => 1,
    Offline => 2
);

impl BeeSerde for TargetReachabilityState {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.u8((*self).into())
    }

    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        des.u8()?.try_into()
    }
}

/// The BeeGFS authentication secret
///
/// Sent by the `AuthenticateChannel` message to authenticate a connection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct AuthenticationSecret(i64);

impl AuthenticationSecret {
    pub fn from_bytes(str: impl AsRef<[u8]>) -> Self {
        let (high, low) = str.as_ref().split_at(str.as_ref().len() / 2);
        let high = hsieh::hash(high) as i64;
        let low = hsieh::hash(low) as i64;

        let hash = (high << 32) | low;

        Self(hash)
    }
}
