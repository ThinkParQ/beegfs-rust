#[cfg(feature = "sqlite")]
use rusqlite::ToSql;
#[cfg(feature = "sqlite")]
use rusqlite::types::FromSql;
use std::fmt::Display;
use std::hash::Hash;
use std::sync::Arc;

pub const MGMTD_UID: Uid = Uid { uid: 1, info: None };

#[derive(Debug, Default, Clone, Eq)]
pub struct Uid {
    uid: i64,
    info: Option<Arc<str>>,
}

impl Uid {
    pub fn with_info(uid: i64, info: impl Into<Arc<str>>) -> Self {
        Self {
            uid,
            info: Some(info.into()),
        }
    }

    pub fn raw(&self) -> i64 {
        self.uid
    }
}

impl PartialEq for Uid {
    fn eq(&self, other: &Self) -> bool {
        self.uid == other.uid
    }
}

impl PartialOrd for Uid {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.uid.partial_cmp(&other.uid)
    }
}

impl Hash for Uid {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.uid.hash(state);
    }
}

impl Display for Uid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(ref info) = self.info {
            Display::fmt(info, f)
        } else {
            write!(f, "uid:{}", self.uid)
        }
    }
}

impl From<i64> for Uid {
    fn from(value: i64) -> Self {
        Uid {
            uid: value,
            info: None,
        }
    }
}
impl From<Uid> for i64 {
    fn from(value: Uid) -> Self {
        value.uid
    }
}

#[cfg(feature = "sqlite")]
impl FromSql for Uid {
    fn column_result(value: rusqlite::types::ValueRef) -> rusqlite::types::FromSqlResult<Self> {
        Ok(Uid {
            uid: value.as_i64()?,
            info: None,
        })
    }
}

#[cfg(feature = "sqlite")]
impl ToSql for Uid {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        self.uid.to_sql()
    }
}
