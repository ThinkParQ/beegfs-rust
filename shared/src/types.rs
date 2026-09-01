//! Various BeeGFS type definitions, mainly for use by BeeMsg.

use crate::bee_serde::*;
use anyhow::{Context, Result, anyhow};
use bee_serde_derive::BeeSerde;
use core::hash::Hash;
#[cfg(feature = "grpc")]
use protobuf::beegfs as pb;
use std::fmt::Debug;
use std::str::FromStr;

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

#[cfg(feature = "grpc")]
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

#[cfg(feature = "grpc")]
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
#[cfg(feature = "grpc")]
impl From<NodeTypeServer> for pb::NodeType {
    fn from(value: NodeTypeServer) -> Self {
        match value {
            NodeTypeServer::Meta => Self::Meta,
            NodeTypeServer::Storage => Self::Storage,
        }
    }
}

/// The network interface type as used by BeeMsg
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NicType {
    Rdma,
    #[default]
    Tcp,
}

impl FromStr for NicType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "tcp" => Ok(Self::Tcp),
            "rdma" => Ok(Self::Rdma),
            s => Err(anyhow!("{s} is not a valid nic type")),
        }
    }
}

impl_enum_bee_msg_traits!(NicType, Tcp => 0, Rdma => 2);

impl_enum_user_str! {NicType,
    NicType::Tcp => "tcp",
    NicType::Rdma => "rdma",
}

#[cfg(feature = "grpc")]
impl_enum_protobuf_traits! {NicType => pb::NicType,
    unspecified => pb::NicType::Unspecified,
    NicType::Tcp => pb::NicType::Ethernet,
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

#[cfg(feature = "grpc")]
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

#[cfg(feature = "grpc")]
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

#[cfg(feature = "grpc")]
impl_enum_protobuf_traits! {QuotaIdType=> pb::QuotaIdType,
    unspecified => pb::QuotaIdType::Unspecified,
    QuotaIdType::User => pb::QuotaIdType::User,
    QuotaIdType::Group => pb::QuotaIdType::Group,
}

/// Type of a quota entry as used by BeeMsg
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum QuotaType {
    #[default]
    Space,
    Inode,
}

impl_enum_bee_msg_traits!(QuotaType,
    Space => 1,
    Inode => 2
);

impl_enum_user_str! {QuotaType,
    QuotaType::Space => "space",
    QuotaType::Inode => "inode",
}

#[cfg(feature = "grpc")]
impl_enum_protobuf_traits! {QuotaType=> pb::QuotaType,
    unspecified => pb::QuotaType::Unspecified,
    QuotaType::Space=> pb::QuotaType::Space,
    QuotaType::Inode => pb::QuotaType::Inode,
}

/// A nodes long term X25519 public key.
///
/// Doubles as the nodes identifier in the BeeMsg key exchange: the initiator sends its own public
/// key in the clear and the responder looks it up in the identity list. Selecting by the key itself
/// rather than by a name makes the selector self certifying - it can never point at the wrong key
/// material - and means the lookup *is* the authentication decision.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct StaticPubKey([u8; Self::LEN]);

impl StaticPubKey {
    pub const LEN: usize = 32;

    pub fn as_bytes(&self) -> &[u8; Self::LEN] {
        &self.0
    }
}

impl From<[u8; StaticPubKey::LEN]> for StaticPubKey {
    fn from(bytes: [u8; StaticPubKey::LEN]) -> Self {
        Self(bytes)
    }
}

impl TryFrom<&[u8]> for StaticPubKey {
    type Error = anyhow::Error;

    fn try_from(bytes: &[u8]) -> Result<Self> {
        Ok(Self(bytes.try_into().with_context(|| {
            format!(
                "A public key must be exactly {} bytes, got {}",
                Self::LEN,
                bytes.len()
            )
        })?))
    }
}

/// Hex, so keys are greppable against the management database and pasteable into it.
impl std::fmt::Display for StaticPubKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for StaticPubKey {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        Ok(Self(parse_hex_32(s)?))
    }
}

/// Parses 64 hex characters into 32 bytes. Shared by public keys and by the on disk private key.
pub(crate) fn parse_hex_32(s: &str) -> Result<[u8; 32]> {
    let s = s.trim();
    anyhow::ensure!(
        s.len() == 64,
        "A hex encoded 32 byte key must be 64 characters, got {}",
        s.len()
    );

    let mut key = [0u8; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
            .context("Not a valid hex encoded 32 byte key")?;
    }

    Ok(key)
}

/// The BeeGFS authentication secret
///
/// Sent by the `AuthenticateChannel` message to authenticate a connection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct AuthSecret(u64);

impl AuthSecret {
    /// Generates sha256 hash from the input byte slice and returns the AuthSecret containing the 8
    /// most significant bytes of the hash in little endian order. Matches the behavior of other
    /// implementations.
    pub fn hash_from_bytes(input: impl AsRef<[u8]>) -> Self {
        let digest = ring::digest::digest(&ring::digest::SHA256, input.as_ref());
        let hash: [u8; 8] = digest
            .as_ref()
            .get(0..8)
            .expect("sha256 must output exactly 32 bytes")
            .try_into()
            .expect("sha256 must output exactly 32 bytes");
        // Following our usual schema, we create the hash integer using little endian order
        let hash = u64::from_le_bytes(hash);
        Self(hash)
    }

    /// Extracts the secret from the input byte slice. Not to be confused with hash_from_bytes(),
    /// which hashes arbitrary input. This function expects a certain format, currently a
    /// stringified i64. Returns None if operation fails.
    pub fn try_from_bytes(input: impl AsRef<[u8]>) -> Result<Self> {
        let str = std::str::from_utf8(input.as_ref())?;
        let value = str.parse()?;

        Ok(Self(value))
    }
}

impl FromStr for AuthSecret {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        Ok(Self(s.parse().with_context(|| {
            format!("{s} does not represent a valid authentication secret string")
        })?))
    }
}
