//! Contains types used by the local database and config.

use protobuf::management as pm;
use rusqlite::{Row, RowIndex};
use shared::impl_enum_protobuf_traits;
use shared::types::*;

mod entity;
pub(crate) use entity::*;

/// Defines methods to convert a type to or from a string representation used in the sqlite database
pub(crate) trait SqliteEnumExt {
    fn sql_variant(&self) -> i64;
    fn from_sql_variant(s: i64) -> rusqlite::Result<Self>
    where
        Self: Sized;

    fn from_row(row: &Row, idx: impl RowIndex) -> rusqlite::Result<Self>
    where
        Self: Sized,
    {
        Self::from_sql_variant(row.get_ref(idx)?.as_i64()?)
    }
}

/// Implements SqliteStr for an enum
macro_rules! impl_enum_sqlite {
    ($type:ty, $($variant:path=> $text:literal),+ $(,)?) => {
        impl SqliteEnumExt for $type {
            fn sql_variant(&self) -> i64 {
                match self {
                    $(
                        $variant => $text,
                    )+
                }
            }

            fn from_sql_variant(s: i64) -> ::rusqlite::Result<Self>
            where
                Self: Sized,
            {
                match s {
                    $(
                        $text => Ok($variant),
                    )+
                    _ => Err(::rusqlite::Error::from(::rusqlite::types::FromSqlError::InvalidType)),
                }
            }
        }
    };
}

impl_enum_sqlite! {EntityType,
    EntityType::Node => 1,
    EntityType::Target => 2,
    EntityType::Pool => 3,
    EntityType::BuddyGroup => 4
}

impl_enum_sqlite! {NodeType,
    NodeType::Meta => 1,
    NodeType::Storage => 2,
    NodeType::Client => 3,
    NodeType::Management => 4
}

impl_enum_sqlite! {NodeTypeServer,
    NodeTypeServer::Meta => 1,
    NodeTypeServer::Storage => 2,
}

impl_enum_sqlite! {NicType,
    NicType::Tcp => 1,
    NicType::Rdma => 2,
}

impl_enum_sqlite! {TargetConsistencyState,
    TargetConsistencyState::Good => 1,
    TargetConsistencyState::NeedsResync => 2,
    TargetConsistencyState::Bad => 3,
}

impl_enum_sqlite! {QuotaIdType,
    QuotaIdType::User => 1,
    QuotaIdType::Group => 2,
}

impl_enum_sqlite! {QuotaType,
    QuotaType::Space => 1,
    QuotaType::Inode => 2,
}

/// How to handle quota data on targets that are part of a buddy group
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum BuddyGroupQuotaAccounting {
    Primary,
    Both,
}

impl_enum_sqlite! {BuddyGroupQuotaAccounting,
    BuddyGroupQuotaAccounting::Primary => 1,
    BuddyGroupQuotaAccounting::Both => 2,
}

use pm::buddy_group_options::BuddyGroupQuotaAccounting as BGQ;
impl_enum_protobuf_traits! {BuddyGroupQuotaAccounting => BGQ,
    unspecified => BGQ::Unspecified,
    BuddyGroupQuotaAccounting::Primary => BGQ::Primary,
    BuddyGroupQuotaAccounting::Both => BGQ::Both,
}
