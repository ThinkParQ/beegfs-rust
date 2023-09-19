use super::msg_buf::MsgBuf;
use super::stream::Stream;
use crate::msg::{Msg, MsgID};
use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;

/// Enables an object to act as a message dispatcher and being called from the generic connection
/// pool.
#[async_trait]
pub trait DispatchRequest: Clone + Debug + Send + Sync + 'static {
    async fn dispatch_request(&self, chn: impl Request) -> Result<()>;
}

#[async_trait]
pub trait Request: Send + Sync {
    async fn respond(self, msg: &impl Msg) -> Result<()>;
    fn authenticate_connection(&mut self);
    fn addr(&self) -> SocketAddr;
    fn msg_id(&self) -> MsgID;
    fn deserialize_msg<M: Msg>(&self) -> Result<M>;
}

#[derive(Debug)]
pub struct StreamRequest<'a> {
    pub(super) stream: &'a mut Stream,
    pub(super) buf: &'a mut MsgBuf,
}

#[async_trait]
impl<'a> Request for StreamRequest<'a> {
    async fn respond(mut self, msg: &impl Msg) -> Result<()> {
        self.buf.serialize_msg(msg)?;
        self.buf.write_to_stream(self.stream).await
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

    fn deserialize_msg<M: Msg>(&self) -> Result<M> {
        self.buf.deserialize_msg()
    }

    fn msg_id(&self) -> MsgID {
        self.buf.msg_id()
    }
}

#[derive(Debug)]
pub struct SocketRequest<'a> {
    pub(crate) sock: Arc<UdpSocket>,
    pub(crate) peer_addr: SocketAddr,
    pub(crate) msg_buf: &'a mut MsgBuf,
}

#[async_trait]
impl<'a> Request for SocketRequest<'a> {
    async fn respond(mut self, msg: &impl Msg) -> Result<()> {
        self.msg_buf.serialize_msg(msg)?;

        self.msg_buf
            .send_to_socket(&self.sock, &self.peer_addr)
            .await
    }

    fn authenticate_connection(&mut self) {
        // No authentication mechanism for sockets
    }

    fn addr(&self) -> SocketAddr {
        self.peer_addr
    }

    fn deserialize_msg<M: Msg>(&self) -> Result<M> {
        self.msg_buf.deserialize_msg()
    }

    fn msg_id(&self) -> MsgID {
        self.msg_buf.msg_id()
    }
}
