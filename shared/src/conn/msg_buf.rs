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
pub struct MsgBuf {
    ser_msg: BytesMut,
    header: Box<Option<Header>>,
}

impl MsgBuf {
    pub fn serialize_msg<M: Msg>(&mut self, msg: &M) -> Result<()> {
        self.ser_msg.truncate(0);

        let mut split = SplitBuffers::from(&mut self.ser_msg);

        let mut ser_body = Serializer::new(&mut split.body, msg.build_feature_flags());
        msg.serialize(&mut ser_body)?;

        let header = Header::new(ser_body.bytes_written(), M::ID, msg.build_feature_flags());

        let mut ser_header = Serializer::new(split.header, msg.build_feature_flags());
        header.serialize(&mut ser_header)?;

        self.header.replace(header);
        Ok(())
    }

    pub fn deserialize_msg<M: Msg>(&self) -> Result<M> {
        let mut des = Deserializer::new(
            &self.ser_msg[Header::LEN..],
            self.header
                .as_ref()
                .as_ref()
                .expect("Header field must be set by serializing a message or receiving data first")
                .msg_feature_flags,
        );
        M::deserialize(&mut des)
    }

    pub(super) async fn read_from_stream(&mut self, stream: &mut Stream) -> Result<()> {
        if self.ser_msg.len() < Header::LEN {
            self.ser_msg.resize(Header::LEN, 0);
        }

        stream.read_exact(&mut self.ser_msg[0..Header::LEN]).await?;
        let header = Header::from_buf(&self.ser_msg[0..Header::LEN])?;
        let msg_len = header.msg_len();
        self.header.replace(header);

        if self.ser_msg.len() != msg_len {
            self.ser_msg.resize(msg_len, 0);
        }

        stream
            .read_exact(&mut self.ser_msg[Header::LEN..msg_len])
            .await
    }

    pub(super) async fn write_to_stream(&self, stream: &mut Stream) -> Result<()> {
        let msg_len = self.msg_len();
        stream.write_all(&self.ser_msg[0..msg_len]).await?;
        Ok(())
    }

    pub(super) async fn recv_from_socket(&mut self, sock: &Arc<UdpSocket>) -> Result<SocketAddr> {
        if self.ser_msg.len() != MAX_DATAGRAM_LEN {
            self.ser_msg.resize(MAX_DATAGRAM_LEN, 0);
        }

        match sock.recv_from(&mut self.ser_msg).await {
            Ok(n) => {
                let header = Header::from_buf(&self.ser_msg[0..Header::LEN])?;
                self.ser_msg.truncate(header.msg_len());

                self.header.replace(header);

                Ok(n.1)
            }
            Err(err) => Err(err.into()),
        }
    }

    pub(super) async fn send_to_socket(
        &self,
        sock: &UdpSocket,
        peer_addr: &SocketAddr,
    ) -> Result<()> {
        let msg_len = self.msg_len();
        let sent = sock.send_to(&self.ser_msg, peer_addr).await?;

        assert_eq!(sent, msg_len);

        Ok(())
    }

    pub fn msg_id(&self) -> MsgID {
        self.header
            .as_ref()
            .as_ref()
            .expect("Header field must be set by serializing a message or receiving data first")
            .msg_id
    }

    fn msg_len(&self) -> usize {
        self.header
            .as_ref()
            .as_ref()
            .expect("Header field must be set by serializing a message or receiving data first")
            .msg_len()
    }
}

struct SplitBuffers<'a> {
    header: &'a mut BytesMut,
    body: BytesMut,
}

impl<'a> SplitBuffers<'a> {
    fn from(buf: &'a mut BytesMut) -> Self {
        if buf.capacity() < Header::LEN {
            buf.reserve(Header::LEN);
        }

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
