//! Connection to other BeeGFS nodes

use crate::protocol::Protocol;
use crate::types::AuthSecret;
use anyhow::{Result, ensure};
use std::time::Duration;

mod async_queue;
pub mod incoming;
pub mod msg_dispatch;
pub mod outgoing;
mod store;
mod stream;
#[cfg(test)]
mod test;

/// Connection settings shared by the incoming and outgoing side.
///
/// Shared deliberately: both directions must agree on the protocol, and passing one value around
/// makes disagreeing impossible.
#[derive(Debug, Default)]
pub struct ConnConfig {
    pub protocol: Protocol,
    /// Legacy only: require an `AuthenticateChannel` before accepting any other message.
    pub legacy_auth_required: bool,
    /// Legacy only: secret sent on newly opened outgoing streams.
    pub auth_secret: Option<AuthSecret>,
}

impl ConnConfig {
    /// Rejects impossible combinations. Meant to be called once at startup.
    pub fn check(&self) -> Result<()> {
        ensure!(
            self.protocol.is_legacy() || (self.auth_secret.is_none() && !self.legacy_auth_required),
            "The legacy authentication secret cannot be combined with the {:?} BeeMsg protocol",
            self.protocol
        );

        Ok(())
    }
}

/// Fixed length of the stream / TCP message buffers.
/// Must match the `WORKER_BUF(IN|OUT)_SIZE` value in `Worker.h` in the C++
/// codebase.
pub(crate) const TCP_BUF_LEN: usize = 4 * 1024 * 1024;

/// Fixed length of the datagram / UDP message buffers.
/// Must match the `DGRAMMR_(RECV|SEND)BUF_SIZE` value in `DatagramListener.*` in the C/C++
/// codebase. Must be smaller than TCP_BUF_LEN;
const UDP_BUF_LEN: usize = 65536;

/// Reasonable time limit for most stream operations. Notable exceptions are waiting for responses
/// and connecting a stream.
const GENERIC_STREAM_TIME_LIMIT: Duration = Duration::from_secs(5);
/// Short timeout for connecting so the next nic can be tried quickly if this one doesn't work.
const CONNECT_STREAM_TIME_LIMIT: Duration = Duration::from_secs(2);
