//! Tools and operations related to mgmtds database backend.
//!
//! Managements core functionality is about keeping a consistent system state and providing
//! interfaces to query and update it. This is done by storing all information inside an SQLite
//! database. This module provides the functionality to access it and operations to interact with
//! it. The latter hide the raw SQL and resemble a primitive ORM, defining data models in terms of
//! Rust and interfaces to obtain the data.

mod connection;
mod error;
mod op;
#[cfg(test)]
mod test;

pub use connection::{initialize, Connection};
pub use error::*;
pub use op::*;
