//! This library provides shared code for BeeGFS Rust projects.
//!
//! It contains the network [message definitions](msg), [types] to build them
//! from, the [serializer](bee_serde), [connection](conn) handling and other utilities and
//! definitions.

#[macro_use]
mod impl_macros;

pub mod bee_msg;
pub mod bee_serde;
pub mod conn;
#[cfg(feature = "grpc")]
pub mod grpc;
pub mod journald_logger;
pub mod nic;
pub mod parser;
pub mod run_state;
pub mod types;
