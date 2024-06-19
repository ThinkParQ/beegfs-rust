//! Various BeeGFS type definitions, mainly for use by BeeMsg.

use crate::bee_serde::*;
use anyhow::{anyhow, Result};
use bee_serde_derive::BeeSerde;
use core::hash::Hash;
#[cfg(feature = "protobuf")]
use protobuf::beegfs as pb;
use std::fmt::Debug;

mod entity;
pub use entity::*;

// Type aliases for convenience. Used by BeeGFS messaging and the management.
//
// CAUTION: While most known BeeGFS messages use the aliased integers below for these types, some
// do not. It still has to be checked for each BeeGFS message individually which exact type is
// needed for serialization.

pub type Uid = i64;
pub type TargetId = u16;
pub type BuddyGroupId = u16;
pub type Port = u16;
pub type NodeId = u32;
pub type PoolId = u16;
pub type QuotaId = u32;

pub const MGMTD_ID: NodeId = 1;
pub const MGMTD_UID: Uid = 1;
pub const DEFAULT_STORAGE_POOL: PoolId = 1;

/// The BeeGFS node type
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NodeType {
    #[default]
    Meta,
    Storage,
    Client,
    Management,
}

impl_enum_bee_msg_traits!(NodeType,
    Meta => 1,
    Storage => 2,
    Client => 3,
    Management => 4
);

impl_enum_user_str! {NodeType,
    NodeType::Meta => "meta",
    NodeType::Storage => "storage",
    NodeType::Client => "client",
    NodeType::Management => "management"
}

#[cfg(feature = "protobuf")]
impl_enum_protobuf_traits! {NodeType => pb::NodeType,
    unspecified => pb::NodeType::Unspecified,
    NodeType::Meta => pb::NodeType::Meta,
    NodeType::Storage => pb::NodeType::Storage,
    NodeType::Client => pb::NodeType::Client,
    NodeType::Management => pb::NodeType::Management,
}

/// A node type only accepting server nodes.
///
/// In a lot of operations, only meta or storage makes sense, so we provide this extra enum for
/// that.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeTypeServer {
    Meta,
    Storage,
}

impl_enum_user_str! {NodeTypeServer,
    NodeTypeServer::Meta => "meta",
    NodeTypeServer::Storage => "storage",
}

impl TryFrom<NodeType> for NodeTypeServer {
    type Error = anyhow::Error;

    fn try_from(value: NodeType) -> Result<Self, Self::Error> {
        match value {
            NodeType::Meta => Ok(Self::Meta),
            NodeType::Storage => Ok(Self::Storage),
            t => Err(anyhow!("{t} can not be converted to NodeTypeServer")),
        }
    }
}
impl From<NodeTypeServer> for NodeType {
    fn from(value: NodeTypeServer) -> Self {
        match value {
            NodeTypeServer::Meta => Self::Meta,
            NodeTypeServer::Storage => Self::Storage,
        }
    }
}

#[cfg(feature = "protobuf")]
impl TryFrom<pb::NodeType> for NodeTypeServer {
    type Error = anyhow::Error;

    fn try_from(value: pb::NodeType) -> Result<Self, Self::Error> {
        match value {
            pb::NodeType::Meta => Ok(Self::Meta),
            pb::NodeType::Storage => Ok(Self::Storage),
            pb::NodeType::Client | pb::NodeType::Management => {
                Err(anyhow!("NodeTypeServer only allows Meta or Storage"))
            }
            pb::NodeType::Unspecified => Err(anyhow!("NodeType is unspecified")),
        }
    }
}
#[cfg(feature = "protobuf")]
impl From<NodeTypeServer> for pb::NodeType {
    fn from(value: NodeTypeServer) -> Self {
        match value {
            NodeTypeServer::Meta => Self::Meta,
            NodeTypeServer::Storage => Self::Storage,
        }
    }
}

/// The network interface type as used by BeeMsg
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NicType {
    #[default]
    Ethernet,
    Rdma,
}

impl_enum_bee_msg_traits!(NicType, Ethernet => 0, Rdma => 1);

impl_enum_user_str! {NicType,
    NicType::Ethernet => "meta",
    NicType::Rdma => "storage",
}

#[cfg(feature = "protobuf")]
impl_enum_protobuf_traits! {NicType => pb::NicType,
    unspecified => pb::NicType::Unspecified,
    NicType::Ethernet => pb::NicType::Ethernet,
    NicType::Rdma => pb::NicType::Rdma,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CapacityPool {
    Normal,
    Low,
    Emergency,
}

// Defines which pool maps to which index in the response below
impl_enum_bee_msg_traits!(CapacityPool, Normal => 0, Low => 1, Emergency => 2);

impl CapacityPool {
    pub fn bee_msg_vec_index(&self) -> usize {
        match self {
            CapacityPool::Normal => 0,
            CapacityPool::Low => 1,
            CapacityPool::Emergency => 2,
        }
    }
}

impl_enum_user_str! {CapacityPool,
    CapacityPool::Normal => "normal",
    CapacityPool::Low => "low",
    CapacityPool::Emergency=> "emergency",
}

#[cfg(feature = "protobuf")]
impl_enum_protobuf_traits! {CapacityPool => pb::CapacityPool,
    unspecified => pb::CapacityPool::Unspecified,
    CapacityPool::Normal => pb::CapacityPool::Normal,
    CapacityPool::Low => pb::CapacityPool::Low,
    CapacityPool::Emergency=> pb::CapacityPool::Emergency,
}

/// The consistency state of a target as used by BeeMsg
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TargetConsistencyState {
    #[default]
    Good,
    NeedsResync,
    Bad,
}

impl_enum_bee_msg_traits!(TargetConsistencyState,
    Good => 0,
    NeedsResync => 1,
    Bad => 2
);

impl_enum_user_str! {TargetConsistencyState,
    TargetConsistencyState::Good => "good",
    TargetConsistencyState::NeedsResync => "needs_resync",
    TargetConsistencyState::Bad => "bad",
}

#[cfg(feature = "protobuf")]
impl_enum_protobuf_traits! {TargetConsistencyState => pb::ConsistencyState,
    unspecified => pb::ConsistencyState::Unspecified,
    TargetConsistencyState::Good => pb::ConsistencyState::Good,
    TargetConsistencyState::NeedsResync => pb::ConsistencyState::NeedsResync,
    TargetConsistencyState::Bad => pb::ConsistencyState::Bad,
}

impl Serializable for TargetConsistencyState {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.u8((*self).into_bee_serde())
    }
}

impl Deserializable for TargetConsistencyState {
    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        Self::try_from_bee_serde(des.u8()?)
    }
}

/// A node type only accepting server nodes.

/// Type of a quota ID as used by BeeMsg
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum QuotaIdType {
    #[default]
    User,
    Group,
}

impl_enum_bee_msg_traits!(QuotaIdType,
    User => 1,
    Group => 2
);

impl_enum_user_str! {QuotaIdType,
    QuotaIdType::User => "user",
    QuotaIdType::Group => "group",
}

/// Type of a quota entry as used by BeeMsg
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum QuotaType {
    #[default]
    Space,
    Inodes,
}

impl_enum_bee_msg_traits!(QuotaType,
    Space => 1,
    Inodes => 2
);

impl_enum_user_str! {QuotaType,
    QuotaType::Space => "space",
    QuotaType::Inodes => "inodes",
}

/// The BeeGFS authentication secret
///
/// Sent by the `AuthenticateChannel` message to authenticate a connection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct AuthSecret(i64);

impl AuthSecret {
    pub fn from_bytes(str: impl AsRef<[u8]>) -> Self {
        let (high, low) = str.as_ref().split_at(str.as_ref().len() / 2);
        let high = hsieh::hash(high) as i64;
        let low = hsieh::hash(low) as i64;

        let hash = (high << 32) | low;

        Self(hash)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_auth_secret() {
        let expect = ((hsieh::hash(b"hhhhh") as u64) << 32) | hsieh::hash(b"lllll") as u64;
        assert_eq!(expect as i64, AuthSecret::from_bytes(b"hhhhhlllll").0);
        let expect = ((hsieh::hash(b"hhhh") as u64) << 32) | hsieh::hash(b"lllll") as u64;
        assert_eq!(expect as i64, AuthSecret::from_bytes(b"hhhhlllll").0);
    }
}
