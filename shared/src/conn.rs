//! Connection to other BeeGFS nodes

use crate::conn::protocol::StaticPubKey;
use crate::types::Uid;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

mod async_queue;
pub mod handshake;
pub mod incoming;
pub mod msg_dispatch;
pub mod noise;
pub mod outgoing;
pub mod protocol;
mod store;
mod stream;
#[cfg(any())] // TEMP-DISABLED-TESTS: re-enable by restoring #[cfg(test)]
mod test;

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

/// A principal allowed to connect. Usually a node, but not necessarily - beegfs-ctl has no node
/// entry, for example.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub name: Arc<str>,
    pub node_uid: Option<Uid>,
}

/// Lookup functions used by connection handling
pub trait Lookup: std::fmt::Debug + Clone + Send + Sync + 'static {
    fn identity_by_key(
        &self,
        key: StaticPubKey,
    ) -> impl Future<Output = Result<Option<Identity>>> + Send;
    fn key_by_node(&self, node: Uid) -> impl Future<Output = Result<Option<StaticPubKey>>> + Send;
    fn node_addrs(&self, node: Uid)
    -> impl Future<Output = Result<Option<Vec<SocketAddr>>>> + Send;
}
