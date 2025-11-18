//! Facilities for dispatching TCP and UDP messages to their message handlers

use super::stream::Stream;
use crate::bee_msg::{Header, Msg, MsgId, deserialize_body, serialize};
use crate::bee_serde::{Deserializable, Serializable};
use anyhow::Result;
use std::fmt::Debug;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;

/// Enables an object to act as a message dispatcher and being called from the generic connection
/// pool.
pub trait DispatchRequest: Clone + Debug + Send + Sync + 'static {
    fn dispatch_request(&self, chn: impl Request) -> impl Future<Output = Result<()>> + Send;
}

/// Defines the required functionality of the object containing the request data (e.g. message and
/// peer).
///
/// Abstracts away the underlying protocol (TCP or UDP), so the message handler doesn't need
/// to know about that.
pub trait Request: Send + Sync {
    fn respond<M: Msg + Serializable>(self, msg: &M) -> impl Future<Output = Result<()>> + Send;
    fn authenticate_connection(&mut self);
    fn addr(&self) -> SocketAddr;
    fn msg_id(&self) -> MsgId;
    fn deserialize_msg<M: Msg + Deserializable>(&self) -> Result<M>;
}

/// Represents a request made via a TCP stream
#[derive(Debug)]
pub struct StreamRequest<'a> {
    pub(super) stream: &'a mut Stream,
    pub(super) buf: &'a mut [u8],
    pub header: &'a Header,
}

impl Request for StreamRequest<'_> {
    async fn respond<M: Msg + Serializable>(self, msg: &M) -> Result<()> {
        let msg_len = serialize(msg, self.buf)?;
        self.stream.write_all(&self.buf[0..msg_len]).await
    }

    fn authenticate_connection(&mut self) {
        if !self.stream.authenticated {
            log::debug!(
                "Marking stream from {:?} as authenticated",
                self.stream.addr()
            );
            self.stream.authenticated = true;
        }
    }

    fn addr(&self) -> SocketAddr {
        self.stream.addr()
    }

    fn deserialize_msg<M: Msg + Deserializable>(&self) -> Result<M> {
        deserialize_body(self.header, &self.buf[Header::LEN..])
    }

    fn msg_id(&self) -> MsgId {
        self.header.msg_id()
    }
}

/// Represents a request made via a UDP datagram
#[derive(Debug)]
pub struct SocketRequest<'a> {
    pub(crate) sock: Arc<UdpSocket>,
    pub(crate) peer_addr: SocketAddr,
    pub(crate) buf: &'a mut [u8],
    pub header: &'a Header,
}

impl Request for SocketRequest<'_> {
    async fn respond<M: Msg + Serializable>(self, msg: &M) -> Result<()> {
        let msg_len = serialize(msg, self.buf)?;
        self.sock
            .send_to(&self.buf[0..msg_len], &self.peer_addr)
            .await?;
        Ok(())
    }

    fn authenticate_connection(&mut self) {
        // No authentication mechanism for sockets
    }

    fn addr(&self) -> SocketAddr {
        self.peer_addr
    }

    fn deserialize_msg<M: Msg + Deserializable>(&self) -> Result<M> {
        deserialize_body(self.header, &self.buf[Header::LEN..])
    }

    fn msg_id(&self) -> MsgId {
        self.header.msg_id()
    }
}
