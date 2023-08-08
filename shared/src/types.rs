//! Various BeeGFS type definitions
//!
//! Used internally (e.g. in the management) and by network messages. Types only used by BeeGFS
//! messages are found in [crate::msg::types].

mod buddy_group;
pub use buddy_group::*;
mod nic;
pub use nic::*;
mod node;
pub use node::*;
mod pool;
pub use pool::*;
mod quota;
pub use quota::*;
mod target;

use crate::bee_serde::{self, *};
use crate::{impl_enum_to_int, impl_enum_to_sql_str, impl_enum_to_user_str, impl_newtype_to_sql};
use anyhow::{bail, Result};
use bee_macro::BeeSerde;
use core::hash::Hash;
use std::fmt::{Debug, Display};
use std::string::FromUtf8Error;
pub use target::*;
use thiserror::Error;

/// Implements SQLite support for a newtype.
///
/// Types with SQLite support can be used as parameters in SQLite statements without converting
/// them to a compatible type first.
///
/// The inner type must implement [rusqlite::types::ToSql] and [rusqlite::types::FromSql].
#[macro_export]
macro_rules! impl_newtype_to_sql {
    ($type:ty => $inner:ty) => {
        impl ::rusqlite::types::ToSql for $type {
            fn to_sql(&self) -> ::rusqlite::Result<::rusqlite::types::ToSqlOutput> {
                self.0.to_sql()
            }
        }

        impl ::rusqlite::types::FromSql for $type {
            fn column_result(
                value: ::rusqlite::types::ValueRef,
            ) -> ::rusqlite::types::FromSqlResult<Self> {
                Ok(Self(<$inner>::column_result(value)?))
            }
        }
    };
}

/// Implements safe (e.g. no panic) conversion of an enum to all integer types and back
#[macro_export]
macro_rules! impl_enum_to_int {
    ($type:ty, $($variant:ident => $number:literal),+) => {
        impl_enum_to_int!(INT_VARIANT $type => u8, $($variant => $number),+);
        impl_enum_to_int!(INT_VARIANT $type => u16, $($variant => $number),+);
        impl_enum_to_int!(INT_VARIANT $type => i16, $($variant => $number),+);
        impl_enum_to_int!(INT_VARIANT $type => u32, $($variant => $number),+);
        impl_enum_to_int!(INT_VARIANT $type => i32, $($variant => $number),+);
        impl_enum_to_int!(INT_VARIANT $type => u64, $($variant => $number),+);
        impl_enum_to_int!(INT_VARIANT $type => i64, $($variant => $number),+);
        impl_enum_to_int!(INT_VARIANT $type => usize, $($variant => $number),+);
        impl_enum_to_int!(INT_VARIANT $type => isize, $($variant => $number),+);
    };

    (INT_VARIANT $type:ty => $int_type:ty, $($variant:ident => $number:literal),+) => {
        impl TryFrom<$int_type> for $type {
            type Error = ::anyhow::Error;
            fn try_from(value: $int_type) -> Result<Self, Self::Error> {
                match value {
                    $(
                        $number => Ok(Self::$variant),
                    )+
                    t => Err(::anyhow::anyhow!($crate::types::InvalidEnumValue(t))),
                }
            }
        }

        impl From<$type> for $int_type {
            fn from(value: $type) -> $int_type {
                match value {
                    $(
                        <$type>::$variant => $number,
                    )+
                }
            }
        }
    };
}

/// Implements SQLite support for an enum (without data) by converting its variants into strings.
///
/// The enum can then be used as parameter for a TEXT column.
#[macro_export]
macro_rules! impl_enum_to_sql_str {
    ($type:ty, $($variant:ident => $text:literal),+ $(,)?) => {

        impl $type {
            pub fn as_sql_str(&self) -> &'static str {
                match self {
                    $(
                        Self::$variant => $text,
                    )+
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

/// Implements a user friendly string representation of the enum.
///
/// Provides `as_user_str()` and implements [Display], which give a `'static str` representing the
/// enums variant. Also implements `FromStr` to allow the enum to be parsed from user input.
#[macro_export]
macro_rules! impl_enum_to_user_str {
    ($type:ty, $($variant:ident => $text:literal),+) => {
        impl $type {
            fn as_user_str(&self) -> &'static str {
                match self {
                    $(
                        Self::$variant => $text,
                    )+
                }
            }
        }

        impl ::std::str::FromStr for $type {
            type Err = ::anyhow::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let s = s.to_lowercase();

                match s.as_str() {
                    $(
                        $text => Ok(Self::$variant),
                    )+
                    t => Err(anyhow::anyhow!("Invalid enum value {t} for conversion")),
                }
            }
        }

        impl ::std::fmt::Display for $type {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                write!(f, "{}", self.as_user_str())
            }
        }
    };
}

/// A globally unique entity identifier.
///
/// "Globally" means spanning different entity types, e.g. nodes, targets, buddy groups and storage
/// pools.
///
/// It's based on an i64 because the management as the authorative instance giving these out uses
/// SQLite as a backend. SQLite only supports signed 64bit integers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityUID(i64);

impl_newtype_to_sql!(EntityUID => i64);

impl Display for EntityUID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl From<i64> for EntityUID {
    fn from(value: i64) -> Self {
        Self(value)
    }
}

/// Returned when integer or string fails the enum conversion (doesn't match any variant)
#[derive(Clone, Debug, Error)]
#[error("Invalid enum value {0} for conversion")]
pub struct InvalidEnumValue<I: Display>(pub I);

/// Matches the `FhgfsOpsErr` value from the BeeGFS C/C++ codebase.
///
/// Since this is a struct, not an enum, it allows for all possible values, not only the ones
/// defined as constants.
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

/// The BeeGFS message ID as defined in `NetMsgTypes.h`
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct MsgID(pub u16);

impl From<u16> for MsgID {
    fn from(value: u16) -> Self {
        Self(value)
    }
}

impl From<MsgID> for u16 {
    fn from(value: MsgID) -> u16 {
        value.0
    }
}

impl Display for MsgID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The BeeGFS AckID
///
/// AckID is always a c string.
#[derive(Clone, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct AckID(#[bee_serde(as = CStr<0>)] Vec<u8>);

impl Debug for AckID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("AckID")
            .field(&String::from_utf8_lossy(&self.0))
            .finish()
    }
}

impl From<&str> for AckID {
    fn from(s: &str) -> Self {
        Self(s.as_bytes().to_owned())
    }
}

impl From<Vec<u8>> for AckID {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

impl AsRef<[u8]> for AckID {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

/// The BeeGFS generic response code
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct GenericResponseCode(i32);

impl GenericResponseCode {
    pub const TRY_AGAIN: Self = Self(0);
    pub const INDIRECT_COMM_ERR: Self = Self(1);
    pub const NEW_SEQ_NO_BASE: Self = Self(2);
}

/// A user friendly alias for an entity
///
/// Aliases an [EntityUID]. Meant to be used in command line arguments and config files to identify
/// entities.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct EntityAlias(String);

impl Display for EntityAlias {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for EntityAlias {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl From<String> for EntityAlias {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl TryFrom<Vec<u8>> for EntityAlias {
    type Error = FromUtf8Error;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        Ok(Self(String::from_utf8(value)?))
    }
}

impl AsRef<[u8]> for EntityAlias {
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl_newtype_to_sql!(EntityAlias => String);

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
