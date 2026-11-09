//! Connection to other BeeGFS nodes

use beecrypt::{KeyPair, KeyPairId, Mode};
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;

mod async_queue;
pub mod incoming;
pub mod msg_dispatch;
pub mod outgoing;
mod store;
mod stream;

pub use beecrypt;

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

pub type IdentityId = i64;

#[derive(Clone)]
pub struct ConnCrypto {
    mode: Mode,
    keypair: Arc<KeyPair>,
}

impl ConnCrypto {
    pub fn new(mode: Mode, keypair: KeyPair) -> Self {
        Self {
            mode,
            keypair: Arc::new(keypair),
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn key_id(&self) -> KeyPairId {
        self.keypair.id()
    }

    pub fn grpc_server_config(&self) -> beecrypt::Result<Arc<beecrypt::rustls::ServerConfig>> {
        self.keypair.grpc_server_config()
    }
}

impl Debug for ConnCrypto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnCrypto")
            .field("mode", &self.mode)
            .field("key_id", &self.keypair.id().to_string())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerIdentity {
    Legacy,
    Unregistered(KeyPairId),
    Registered {
        key_id: KeyPairId,
        identity_id: IdentityId,
    },
}
