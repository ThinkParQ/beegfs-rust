use crate::bee_serde::{Deserializable, Deserializer, Serializable, Serializer};
#[cfg(feature = "sqlite")]
use rusqlite::ToSql;
#[cfg(feature = "sqlite")]
use rusqlite::types::{FromSql, Value};
use std::fmt::Display;
use std::hash::Hash;
use std::sync::Arc;

pub const DEFAULT_STORAGE_POOL: PoolId = PoolId { id: 1, info: None };

#[derive(Debug, Default, Clone, Eq)]
pub struct PoolId {
    id: u16,
    info: Option<Arc<str>>,
}

impl PoolId {
    pub fn with_info(id: u16, info: impl Into<Arc<str>>) -> Self {
        Self {
            id,
            info: Some(info.into()),
        }
    }

    pub fn raw(&self) -> u16 {
        self.id
    }

    pub fn is_zero(&self) -> bool {
        self.id == 0
    }
}

impl PartialEq for PoolId {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl PartialOrd for PoolId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.id.partial_cmp(&other.id)
    }
}

impl Hash for PoolId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Display for PoolId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(ref info) = self.info {
            Display::fmt(info, f)
        } else {
            write!(f, "pool_id:{}", self.id)
        }
    }
}

impl From<u16> for PoolId {
    fn from(value: u16) -> Self {
        PoolId {
            id: value,
            info: None,
        }
    }
}
impl TryFrom<u32> for PoolId {
    type Error = anyhow::Error;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.try_into()?,
            info: None,
        })
    }
}
impl TryFrom<i32> for PoolId {
    type Error = anyhow::Error;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.try_into()?,
            info: None,
        })
    }
}

impl Serializable for PoolId {
    fn serialize(&self, ser: &mut Serializer) -> anyhow::Result<()> {
        self.id.serialize(ser)
    }
}

impl Deserializable for PoolId {
    fn deserialize(des: &mut Deserializer) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        Ok(Self {
            id: Deserializable::deserialize(des)?,
            info: None,
        })
    }
}

#[cfg(feature = "sqlite")]
impl FromSql for PoolId {
    fn column_result(value: rusqlite::types::ValueRef) -> rusqlite::types::FromSqlResult<Self> {
        let id = value.as_i64()?;
        let id = id
            .try_into()
            .map_err(|_| rusqlite::types::FromSqlError::OutOfRange(id))?;
        Ok(PoolId { id, info: None })
    }
}

#[cfg(feature = "sqlite")]
impl ToSql for PoolId {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        self.id.to_sql()
    }
}

#[cfg(feature = "sqlite")]
impl From<PoolId> for Value {
    fn from(value: PoolId) -> Self {
        Value::Integer(value.id.into())
    }
}
