use super::msg_buffer::MsgBuffer;
use super::stream::Stream;
use crate::conn::PeerID;
use crate::msg::Msg;
use crate::MsgID;
use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;

#[async_trait]
pub trait DispatchRequest: Clone + Debug + Send + Sync + 'static {
    async fn dispatch_request(&mut self, chn: impl RequestChannel + DeserializeMsg) -> Result<()>;
}

#[async_trait]
pub trait RequestChannel: Send + Sync {
    async fn respond(mut self, msg: &impl Msg) -> Result<()>;
    fn authenticate(&mut self);
    fn peer(&self) -> PeerID;
}

pub trait DeserializeMsg {
    fn deserialize_msg<M: Msg>(&self) -> Result<M>;
    fn msg_id(&self) -> MsgID;
}

#[derive(Debug)]
pub struct StreamRequestChannel<'a> {
    pub(super) stream: &'a mut Stream,
    pub(super) msg_buf: &'a mut MsgBuffer,
}

#[async_trait]
impl<'a> RequestChannel for StreamRequestChannel<'a> {
    async fn respond(mut self, msg: &impl Msg) -> Result<()> {
        self.msg_buf.serialize_msg(msg)?;
        self.msg_buf.write_to_stream(self.stream).await
    }

    fn authenticate(&mut self) {
        if !self.stream.authenticated {
            log::debug!(
                "Marking stream from {:?} as authenticated",
                self.stream.peer_id
            );
            self.stream.authenticated = true;
        }
    }

    fn peer(&self) -> PeerID {
        self.stream.peer_id
    }
}

impl<'a> DeserializeMsg for StreamRequestChannel<'a> {
    fn deserialize_msg<M: Msg>(&self) -> Result<M> {
        self.msg_buf.deserialize_msg()
    }

    fn msg_id(&self) -> MsgID {
        self.msg_buf.msg_id()
    }
}

#[derive(Debug)]
pub struct SocketRequestChannel<'a> {
    pub(crate) sock: Arc<UdpSocket>,
    pub(crate) peer_addr: SocketAddr,
    pub(crate) msg_buf: &'a mut MsgBuffer,
}

#[async_trait]
impl<'a> RequestChannel for SocketRequestChannel<'a> {
    async fn respond(mut self, msg: &impl Msg) -> Result<()> {
        self.msg_buf.serialize_msg(msg)?;

        self.msg_buf
            .send_to_socket(&self.sock, self.peer_addr)
            .await
    }

    fn authenticate(&mut self) {
        // No authentication mechanism for sockets
    }

    fn peer(&self) -> PeerID {
        PeerID::Addr(self.peer_addr)
    }
}

impl<'a> DeserializeMsg for SocketRequestChannel<'a> {
    fn deserialize_msg<M: Msg>(&self) -> Result<M> {
        self.msg_buf.deserialize_msg()
    }

    fn msg_id(&self) -> MsgID {
        self.msg_buf.msg_id()
    }
}
