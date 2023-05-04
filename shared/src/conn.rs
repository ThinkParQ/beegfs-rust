mod msg_buffer;
pub mod msg_dispatch;
mod pool;
mod stream;

pub use self::msg_buffer::MsgBuffer;
use crate::NodeUID;
use anyhow::{bail, Result};
use async_trait::async_trait;
pub use pool::*;
use std::fmt::Debug;
use std::hash::Hash;
use std::net::SocketAddr;

#[async_trait]
pub trait AddrResolver: Clone + Debug + Send + Sync + 'static {
    async fn lookup(&self, peer_id: PeerID) -> Result<Vec<SocketAddr>>;
    async fn reverse_lookup(&self, addr: SocketAddr) -> PeerID;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PeerID {
    Addr(SocketAddr),
    Node(NodeUID),
}

#[derive(Clone, Debug)]
pub struct SocketAddrResolver {}

#[async_trait]
impl AddrResolver for SocketAddrResolver {
    async fn lookup(&self, peer_id: PeerID) -> Result<Vec<SocketAddr>> {
        let addr = match peer_id {
            PeerID::Addr(addr) => addr,
            PeerID::Node(_) => bail!("Can't resolve from NodeUID"),
        };

        Ok(vec![addr])
    }
    async fn reverse_lookup(&self, addr: SocketAddr) -> PeerID {
        PeerID::Addr(addr)
    }
}
