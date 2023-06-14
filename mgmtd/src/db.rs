//! The interface to managements SQL backend.
//!
//! Managements core functionality is about keeping a consistent system state and providing
//! interfaces to query and update it. This is done by storing all information inside an SQLite
//! database. This module provides a somehow abstracted access to that database, so the message
//! handlers and other components do not need to use raw SQL.
//!
//! The advantages are :
//! * We avoid duplication. A good part of the queries can be reused.
//! * The queries can be tested against a set of test data
//!
//! # How does it work
//! TODO
//! # Examples
//! TODO
//!
//! # Why no ORM?
//! An ORM would provide compile time checked queries and a much better abstraction than this (which
//! basically provides its own poor mans ORM). But on the over hand, it makes things
//! overcomplicated. I tried [diesel](https://diesel.rs) and looked at others - all of them make
//! some things easier, some harder and more complicated. Not worth it in my opinion.

mod connection;
mod error;
mod op;
#[cfg(test)]
mod test;

pub use connection::{initialize, Connection};
pub use error::*;
pub use op::*;
