use super::stream::Stream;
use crate::bee_serde::{BeeSerde, Deserializer, Serializer};
use crate::msg::{Header, Msg};
use crate::MsgID;
use anyhow::Result;
use bytes::BytesMut;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;

const MAX_DATAGRAM_LEN: usize = 65535;

#[derive(Debug, Default)]
pub struct MsgBuffer {
    buf: BytesMut,
    des_header: Box<Option<Header>>,
}

impl MsgBuffer {
    pub fn serialize_msg<M: Msg>(&mut self, msg: &M) -> Result<()> {
        self.buf.truncate(0);

        let mut split = SplitBuffers::from(&mut self.buf);

        let mut ser_body = Serializer::new(&mut split.body, msg.build_feature_flags());
        msg.serialize(&mut ser_body)?;

        let header = Header::new(ser_body.bytes_written(), M::ID, msg.build_feature_flags());

        let mut ser_header = Serializer::new(split.header, msg.build_feature_flags());
        header.serialize(&mut ser_header)?;

        self.des_header.replace(header);
        Ok(())
    }

    pub fn deserialize_msg<M: Msg>(&self) -> Result<M> {
        let mut des = Deserializer::new(
            &self.buf[Header::LEN..],
            self.des_header
                .as_ref()
                .as_ref()
                .expect("No header loaded")
                .msg_feature_flags,
        );
        M::deserialize(&mut des)
    }

    pub(super) async fn read_from_stream(&mut self, stream: &mut Stream) -> Result<()> {
        if self.buf.len() < Header::LEN {
            self.buf.resize(Header::LEN, 0);
        }

        stream.read_exact(&mut self.buf[0..Header::LEN]).await?;

        self.des_header
            .replace(Header::from_buf(&self.buf[0..Header::LEN])?);

        let msg_len = self.msg_len();

        if self.buf.len() != msg_len {
            self.buf.resize(msg_len, 0);
        }

        stream.read_exact(&mut self.buf[Header::LEN..msg_len]).await
    }

    pub(super) async fn write_to_stream(&self, stream: &mut Stream) -> Result<()> {
        let msg_len = self.msg_len();

        // log::warn!(
        //     "WRITE TO STREAM {msg_len} bytes (buf size {})",
        //     self.buf.len()
        // );
        stream.write_all(&self.buf[0..msg_len]).await?;
        Ok(())
    }

    pub(super) async fn recv_from_socket(&mut self, sock: &Arc<UdpSocket>) -> Result<SocketAddr> {
        if self.buf.len() != MAX_DATAGRAM_LEN {
            self.buf.resize(MAX_DATAGRAM_LEN, 0);
        }

        match sock.recv_from(&mut self.buf).await {
            Ok(n) => {
                self.des_header
                    .replace(Header::from_buf(&self.buf[0..Header::LEN])?);

                self.buf
                    .truncate(self.des_header.as_ref().as_ref().unwrap().msg_len());

                Ok(n.1)
            }
            Err(err) => Err(err.into()),
        }
    }

    pub(super) async fn send_to_socket(
        &self,
        sock: &Arc<UdpSocket>,
        peer_addr: SocketAddr,
    ) -> Result<()> {
        let msg_len = self.msg_len();

        // log::warn!(
        //     "SEND TO SOCKET: {msg_len} bytes (buf size {})",
        //     self.buf.len()
        // );

        let sent = sock.send_to(&self.buf, peer_addr).await?;

        assert_eq!(sent, msg_len);

        Ok(())
    }

    pub fn msg_id(&self) -> MsgID {
        self.des_header
            .as_ref()
            .as_ref()
            .expect("No header loaded")
            .msg_id
    }

    fn msg_len(&self) -> usize {
        self.des_header
            .as_ref()
            .as_ref()
            .expect("No header loaded")
            .msg_len()
    }
}

struct SplitBuffers<'a> {
    header: &'a mut BytesMut,
    body: BytesMut,
}

impl<'a> SplitBuffers<'a> {
    fn from(buf: &'a mut BytesMut) -> Self {
        let body = buf.split_off(Header::LEN);
        Self { header: buf, body }
    }
}

impl<'a> Drop for SplitBuffers<'a> {
    fn drop(&mut self) {
        let body = std::mem::take(&mut self.body);
        self.header.unsplit(body);
    }
}
