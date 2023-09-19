//! Distinguishable error type definition

use thiserror::Error;

/// An error type containing reoccuring errors with standard messages.
///
/// Contains information about the nature/type/cause of the error, which can be matched on by the
/// caller to take appropriate action.
#[derive(Debug, Error)]
pub enum TypedError {
    /// A database entry with a field `name` being `value` does not exist.
    #[error("{name} with value(s) {value} not found")]
    ValueNotFound { name: String, value: String },
    /// A database entry with a specific field `name` being `value` does already exist and was
    /// expected not to (e.g. for a new entry).
    #[error("{name} with value {value} already exists")]
    ValueExists { name: String, value: String },
}

impl TypedError {
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
}
