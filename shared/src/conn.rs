//! Connection to other BeeGFS nodes

use std::time::Duration;

mod async_queue;
pub mod incoming;
pub mod msg_dispatch;
pub mod outgoing;
mod store;
mod stream;

/// Fixed length of the stream / TCP message buffers.
/// Must match the `WORKER_BUF(IN|OUT)_SIZE` value in `Worker.h` in the C++
/// codebase.
const TCP_BUF_LEN: usize = 4 * 1024 * 1024;

/// Fixed length of the datagram / UDP message buffers.
/// Must match the `DGRAMMR_(RECV|SEND)BUF_SIZE` value in `DatagramListener.*` in the C/C++
/// codebase. Must be smaller than TCP_BUF_LEN;
const UDP_BUF_LEN: usize = 65536;

/// Reasonable time limit for most stream operations. Notable exceptions are waiting for responses
/// and connecting a stream.
const GENERIC_STREAM_TIME_LIMIT: Duration = Duration::from_secs(5);
/// Short timeout for connecting so the next nic can be tried quickly if this one doesn't work.
const CONNECT_STREAM_TIME_LIMIT: Duration = Duration::from_secs(2);
