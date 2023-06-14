//! Database layers error type definition

use thiserror::Error;

/// The result type used by the database layer.
pub type DbResult<T> = std::result::Result<T, DbError>;

/// The error type used by  the database layer.
///
/// Contains information about the nature/type/cause of the error, which can be matched on by the
/// caller to take appropriate action.
#[derive(Debug, Error)]
pub enum DbError {
    /// A database entry with a field `name` being `value` does not exist.
    #[error("{name} with value(s) {value} not found")]
    ValueNotFound { name: String, value: String },
    /// A database entry with a specific field `name` being `value` does already exist and was
    /// expected not to (e.g. for a new entry).
    #[error("{name} with value {value} already exists")]
    ValueExists { name: String, value: String },
    /// The inner rusqlite crate returned an error.
    #[error(transparent)]
    Sqlite {
        #[from]
        inner: rusqlite::Error,
    },
    /// Generic error without a category.
    #[error("{desc}")]
    Other { desc: String },
}

impl DbError {
    pub fn value_exists(name: impl ToString, value: impl ToString) -> Self {
        Self::ValueExists {
            name: name.to_string(),
            value: value.to_string(),
        }
    }

    pub fn value_not_found(name: impl ToString, value: impl ToString) -> Self {
        Self::ValueNotFound {
            name: name.to_string(),
            value: value.to_string(),
        }
    }

    pub fn other(desc: impl ToString) -> Self {
        Self::Other {
            desc: desc.to_string(),
        }
    }
}

/// Necessary for the log_handle_error! macro which also has to deal with anyhow::Error
impl AsRef<dyn std::error::Error + 'static> for DbError {
    fn as_ref(&self) -> &(dyn std::error::Error + 'static) {
        self
    }
}
