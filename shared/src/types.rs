//! Various BeeGFS type definitions, mainly for use by BeeMsg.

use crate::bee_serde::*;
use anyhow::{anyhow, Result};
use bee_serde_derive::BeeSerde;
use core::hash::Hash;
use std::fmt::Debug;

// Type aliases for convenience. Used by BeeGFS messaging and the management.
//
// CAUTION: While most known BeeGFS messages use the aliased integers below for these types, some
// do not. It still has to be checked for each BeeGFS message individually which exact type is
// needed for serialization.

pub type EntityUID = u64;
pub type TargetID = u16;
pub type BuddyGroupID = u16;
pub type Port = u16;
pub type NodeID = u32;
pub type StoragePoolID = u16;
pub type QuotaID = u32;

pub const MGMTD_ID: NodeID = 1;
pub const MGMTD_UID: EntityUID = 1;

pub const DEFAULT_STORAGE_POOL: StoragePoolID = 1;

/// The node type as used by most BeeGFS messages
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

/// A node type only accepting server nodes.
///
/// In a lot of operations, only meta or storage makes sense, so we provide this extra enum for
/// that.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeTypeServer {
    Meta,
    Storage,
}

impl TryFrom<NodeType> for NodeTypeServer {
    type Error = anyhow::Error;

    fn try_from(value: NodeType) -> Result<Self, Self::Error> {
        match value {
            NodeType::Meta => Ok(Self::Meta),
            NodeType::Storage => Ok(Self::Storage),
            t => Err(anyhow!("{t:?} cannot be converted")),
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

/// The network interface type as used by BeeMsg
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NicType {
    #[default]
    Ethernet,
    Rdma,
}

impl_enum_to_int!(NicType, Ethernet => 0, Rdma => 1);

/// Type of a quota ID as used by BeeMsg
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

/// Type of a quota entry as used by BeeMsg
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

/// The consistency state of a target as used by BeeMsg
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

impl Serializable for TargetConsistencyState {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.u8((*self).into())
    }
}

impl Deserializable for TargetConsistencyState {
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
