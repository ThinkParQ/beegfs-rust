//! Contains types used by the local database and config.

use rusqlite::Row;
use shared::types::*;

mod entity;
pub(crate) use entity::*;

/// Defines methods to convert a type to or from a string representation used in the sqlite database
pub(crate) trait SqliteExt {
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
macro_rules! impl_enum_sqlite {
    ($type:ty, $($variant:path=> $text:literal),+ $(,)?) => {
        impl SqliteExt for $type {
            fn sql_str(&self) -> &str {
                match self {
                    $(
                        $variant => $text,
                    )+
                }
            }

            fn from_sql_str(s: &str) -> ::rusqlite::Result<Self>
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
    EntityType::Node => "node",
    EntityType::Target => "target",
    EntityType::Pool => "pool",
    EntityType::BuddyGroup => "buddy_group"
}

impl_enum_sqlite! {NodeType,
    NodeType::Meta => "meta",
    NodeType::Storage => "storage",
    NodeType::Client => "client",
    NodeType::Management => "management"
}

impl_enum_sqlite! {NodeTypeServer,
    NodeTypeServer::Meta => "meta",
    NodeTypeServer::Storage => "storage",
}

impl_enum_sqlite! {NicType,
    NicType::Ethernet => "ethernet",
    NicType::Rdma => "rdma",
}

impl_enum_sqlite! {TargetConsistencyState,
    TargetConsistencyState::Good => "good",
    TargetConsistencyState::NeedsResync => "needs_resync",
    TargetConsistencyState::Bad => "bad",
}

impl_enum_sqlite! {QuotaIdType,
    QuotaIdType::User => "user",
    QuotaIdType::Group => "group",
}

impl_enum_sqlite! {QuotaType,
    QuotaType::Space => "space",
    QuotaType::Inodes => "inodes",
}

impl SqliteExt for Alias {
    fn sql_str(&self) -> &str {
        self.as_ref()
    }

    fn from_sql_str(s: &str) -> rusqlite::Result<Self>
    where
        Self: Sized,
    {
        Ok(s.try_into()
            .map_err(|_| rusqlite::types::FromSqlError::InvalidType)?)
    }
}
