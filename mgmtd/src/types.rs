//! Contains types used by the local database and config.

use pb::beegfs::beegfs as pb;
use rusqlite::Row;
use shared::bee_msg::misc::CapacityPool;
use shared::types::{
    NicType, NodeType, NodeTypeServer, QuotaIDType, QuotaType, TargetConsistencyState,
};

/// Defines methods to convert a type to or from a string representation used in the sqlite database
pub(crate) trait SqliteStr {
    fn sql_str(&self) -> &str;
    fn from_sql_str(s: &str) -> rusqlite::Result<Self>
    where
        Self: Sized;

    fn from_row(row: &Row, idx: usize) -> rusqlite::Result<Self>
    where
        Self: Sized,
    {
        Self::from_sql_str(row.get_ref(idx)?.as_str()?)
    }
}

/// Implements SqliteStr for an enum
macro_rules! impl_enum_sqlite_str {
    ($type:ty, $($variant:ident => $text:tt),+ $(,)?) => {
        impl SqliteStr for $type {
            fn sql_str(&self) -> &str {
                match self {
                    $(
                        Self::$variant => $text,
                    )+
                }
            }

            fn from_sql_str(s: &str) -> ::rusqlite::Result<Self>
            where
                Self: Sized,
            {
                match s {
                    $(
                        $text => Ok(<$type>::$variant),
                    )+
                    _ => Err(::rusqlite::Error::from(::rusqlite::types::FromSqlError::InvalidType)),
                }
            }
        }
    };
}

pub(crate) const UNSPECIFIED: &str = "<unspecified>";

pub(crate) const NODE_TYPE_META: &str = "meta";
pub(crate) const NODE_TYPE_STORAGE: &str = "storage";
pub(crate) const NODE_TYPE_CLIENT: &str = "client";
pub(crate) const NODE_TYPE_MANAGEMENT: &str = "management";

impl_enum_sqlite_str!(NodeType,
    Meta => NODE_TYPE_META,
    Storage => NODE_TYPE_STORAGE,
    Client => NODE_TYPE_CLIENT,
    Management => NODE_TYPE_MANAGEMENT,
);
impl_enum_sqlite_str!(pb::NodeType,
    Unspecified => UNSPECIFIED,
    Meta => NODE_TYPE_META,
    Storage => NODE_TYPE_STORAGE,
    Client => NODE_TYPE_CLIENT,
    Management => NODE_TYPE_MANAGEMENT,
);
impl_enum_sqlite_str!(NodeTypeServer,
    Meta => NODE_TYPE_META,
    Storage => NODE_TYPE_STORAGE,
);

pub(crate) const NIC_TYPE_ETHERNET: &str = "ethernet";
pub(crate) const NIC_TYPE_RDMA: &str = "rdma";

impl_enum_sqlite_str!(NicType,
    Ethernet => NIC_TYPE_ETHERNET,
    Rdma => NIC_TYPE_RDMA
);
impl_enum_sqlite_str!(pb::NicType,
    Unspecified => UNSPECIFIED,
    Ethernet => NIC_TYPE_ETHERNET,
    Rdma => NIC_TYPE_RDMA
);

pub(crate) const CAPACITY_POOL_NORMAL: &str = "normal";
pub(crate) const CAPACITY_POOL_LOW: &str = "low";
pub(crate) const CAPACITY_POOL_EMERGENCY: &str = "emergency";

impl_enum_sqlite_str!(CapacityPool,
    Normal => CAPACITY_POOL_NORMAL,
    Low => CAPACITY_POOL_LOW,
    Emergency => CAPACITY_POOL_EMERGENCY
);
impl_enum_sqlite_str!(pb::CapacityPool,
    Unspecified => UNSPECIFIED,
    Normal => CAPACITY_POOL_NORMAL,
    Low => CAPACITY_POOL_LOW,
    Emergency => CAPACITY_POOL_EMERGENCY
);

pub(crate) const CONSISTENCY_STATE_GOOD: &str = "good";
pub(crate) const CONSISTENCY_STATE_NEEDS_RESYNC: &str = "needs_resync";
pub(crate) const CONSISTENCY_STATE_BAD: &str = "bad";

impl_enum_sqlite_str!(TargetConsistencyState,
    Good => CONSISTENCY_STATE_GOOD,
    NeedsResync => CONSISTENCY_STATE_NEEDS_RESYNC,
    Bad => CONSISTENCY_STATE_BAD
);
impl_enum_sqlite_str!(pb::ConsistencyState,
    Unspecified => UNSPECIFIED,
    Good => CONSISTENCY_STATE_GOOD,
    NeedsResync => CONSISTENCY_STATE_NEEDS_RESYNC,
    Bad => CONSISTENCY_STATE_BAD
);

pub(crate) const QUOTA_ID_TYPE_USER: &str = "user";
pub(crate) const QUOTA_ID_TYPE_GROUP: &str = "group";

impl_enum_sqlite_str!(QuotaIDType,
    User => QUOTA_ID_TYPE_USER,
    Group => QUOTA_ID_TYPE_GROUP
);

pub(crate) const QUOTA_TYPE_SPACE: &str = "space";
pub(crate) const QUOTA_TYPE_INODES: &str = "inodes";

impl_enum_sqlite_str!(QuotaType,
    Space => QUOTA_TYPE_SPACE,
    Inodes => QUOTA_TYPE_INODES
);

#[derive(Clone, Debug)]
pub(crate) enum EntityType {
    Node,
    Target,
    BuddyGroup,
    StoragePool,
}

impl_enum_sqlite_str!(EntityType,
    Node => "node",
    Target => "target",
    BuddyGroup => "buddy_group",
    StoragePool => "storage_pool",
);
