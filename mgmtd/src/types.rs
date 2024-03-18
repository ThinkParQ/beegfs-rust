//! Contains types used by the local database and config.
//!
//! Some of the enums are duplicates from the shared crate. This is intended since the shared crate
//! types are defined for BeeMsg, including which variant to convert in which integer value. The
//! enums here, on the other hand, contain info on which value to convert them when writing them to
//! the database. This is information local to the management and does therefore not belong into
//! the shared crate. Thus we define extra types here and provide conversion tools (e.g.
//! implementing From / TryFrom).

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use shared::impl_from_and_into;
use shared::parser::integer_with_generic_unit;

/// Implements SQLite support for an enum (without data) by converting its variants into strings.
///
/// The enum can then be used as parameter for a TEXT column.
macro_rules! impl_enum_to_sql_str {
    ($type:ty, $($variant:ident => $text:literal),+ $(,)?) => {

        impl $type {
            #[allow(dead_code)]
            pub(crate) fn as_sql_str(&self) -> &'static str {
                match self {
                    $(
                        Self::$variant => $text,
                    )+
                }
            }
        }


        impl std::str::FromStr for $type {
            type Err = ::rusqlite::types::FromSqlError;

            fn from_str(str: &str) -> Result<Self, Self::Err> {
                match str {
                    $(
                        $text => Ok(<$type>::$variant),
                    )+
                    _ => Err(::rusqlite::types::FromSqlError::InvalidType),
                }
            }
        }

        impl ::rusqlite::types::ToSql for $type {
            fn to_sql(&self) -> ::rusqlite::Result<::rusqlite::types::ToSqlOutput> {
                Ok(::rusqlite::types::ToSqlOutput::Borrowed(
                        ::rusqlite::types::ValueRef::Text(match self {
                            $(
                                Self::$variant => $text.as_bytes(),
                            )+
                        }
                    ),
                ))
            }
        }

        impl ::rusqlite::types::FromSql for $type {
            fn column_result(
                value: ::rusqlite::types::ValueRef,
            ) -> ::rusqlite::types::FromSqlResult<Self> {
                let raw = String::column_result(value)?;

                match raw.as_str() {
                    $(
                        $text => Ok(Self::$variant),
                    )+
                    _ => Err(::rusqlite::types::FromSqlError::InvalidType),
                }
            }
        }
    };
}

/// A node type
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum NodeType {
    Meta,
    Storage,
    Client,
    Management,
}
impl_enum_to_sql_str!(NodeType,
    Meta => "meta",
    Storage => "storage",
    Client => "client",
    Management => "management",
);
impl From<shared::types::NodeType> for NodeType {
    fn from(value: shared::types::NodeType) -> Self {
        match value {
            shared::types::NodeType::Meta => Self::Meta,
            shared::types::NodeType::Storage => Self::Storage,
            shared::types::NodeType::Client => Self::Client,
            shared::types::NodeType::Management => Self::Management,
        }
    }
}
impl From<NodeType> for shared::types::NodeType {
    fn from(value: NodeType) -> Self {
        match value {
            NodeType::Meta => Self::Meta,
            NodeType::Storage => Self::Storage,
            NodeType::Client => Self::Client,
            NodeType::Management => Self::Management,
        }
    }
}

/// A node type only accepting server nodes.
///
/// In a lot of operations, only meta or storage makes sense, so we provide this extra enum for
/// that.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum NodeTypeServer {
    Meta,
    Storage,
}
impl_enum_to_sql_str!(NodeTypeServer,
    Meta => "meta",
    Storage => "storage"
);
impl TryFrom<shared::types::NodeType> for NodeTypeServer {
    type Error = anyhow::Error;

    fn try_from(value: shared::types::NodeType) -> Result<Self, Self::Error> {
        match value {
            shared::types::NodeType::Meta => Ok(Self::Meta),
            shared::types::NodeType::Storage => Ok(Self::Storage),
            t => Err(anyhow!("{t:?} cannot be converted")),
        }
    }
}
impl From<NodeTypeServer> for shared::types::NodeType {
    fn from(value: NodeTypeServer) -> Self {
        match value {
            NodeTypeServer::Meta => Self::Meta,
            NodeTypeServer::Storage => Self::Storage,
        }
    }
}

/// The entity type
#[derive(Clone, Debug)]
pub(crate) enum EntityType {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CapacityPool {
    Normal,
    Low,
    Emergency,
}
impl_enum_to_sql_str!(CapacityPool, Normal => "normal", Low => "low", Emergency => "emergency");

impl From<CapacityPool> for shared::bee_msg::misc::CapacityPool {
    fn from(value: CapacityPool) -> Self {
        use shared::bee_msg::misc::CapacityPool as BMC;
        match value {
            CapacityPool::Normal => BMC::Normal,
            CapacityPool::Low => BMC::Low,
            CapacityPool::Emergency => BMC::Emergency,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum TargetConsistencyState {
    #[default]
    Good,
    NeedsResync,
    Bad,
}
impl_enum_to_sql_str!(TargetConsistencyState,
    Good => "good",
    NeedsResync => "needs_resync",
    Bad => "bad"
);
impl_from_and_into!(TargetConsistencyState, shared::types::TargetConsistencyState, Good <=> Good, NeedsResync <=> NeedsResync, Bad <=> Bad);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum QuotaIDType {
    User,
    Group,
}
impl_enum_to_sql_str!(QuotaIDType,
    User => "user",
    Group => "group"
);
impl_from_and_into!(QuotaIDType, shared::types::QuotaIDType, User <=> User, Group <=> Group);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum QuotaType {
    Space,
    Inodes,
}
impl_enum_to_sql_str!(QuotaType,
    Space => "space",
    Inodes => "inodes"
);
impl_from_and_into!(QuotaType, shared::types::QuotaType, Space <=> Space, Inodes <=> Inodes);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum NicType {
    Ethernet,
    Sdp,
    Rdma,
}
impl_enum_to_sql_str!(NicType, Ethernet => "ethernet", Sdp => "sdp", Rdma => "rdma");
impl_from_and_into!(NicType, shared::types::NicType, Ethernet <=> Ethernet, Sdp <=> Sdp, Rdma <=> Rdma);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapPoolLimits {
    #[serde(with = "integer_with_generic_unit")]
    pub inodes_low: u64,
    #[serde(with = "integer_with_generic_unit")]
    pub inodes_emergency: u64,
    #[serde(with = "integer_with_generic_unit")]
    pub space_low: u64,
    #[serde(with = "integer_with_generic_unit")]
    pub space_emergency: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CapPoolDynamicLimits {
    #[serde(with = "integer_with_generic_unit")]
    pub inodes_normal_threshold: u64,
    #[serde(with = "integer_with_generic_unit")]
    pub inodes_low_threshold: u64,
    #[serde(with = "integer_with_generic_unit")]
    pub space_normal_threshold: u64,
    #[serde(with = "integer_with_generic_unit")]
    pub space_low_threshold: u64,
    #[serde(with = "integer_with_generic_unit")]
    pub inodes_low: u64,
    #[serde(with = "integer_with_generic_unit")]
    pub inodes_emergency: u64,
    #[serde(with = "integer_with_generic_unit")]
    pub space_low: u64,
    #[serde(with = "integer_with_generic_unit")]
    pub space_emergency: u64,
}
