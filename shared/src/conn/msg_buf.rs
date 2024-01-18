//! Reusable buffer for serialized BeeGFS messages
//!
//! This buffer provides the memory and the functionality to (de-)serialize BeeGFS messages from /
//! into and read / write / send / receive from / to streams and UDP sockets.
//!
//! They are meant to be used in two steps:
//! Serialize a message first, then write or send it to the wire.
//! OR
//! Read or receive data from the wire, then deserialize it into a message.
//!
//! # Example: Reading from stream
//! 1. `.read_from_stream()` to read in the data from stream into the buffer
//! 2. `.deserialize_msg()` to deserialize the message from the buffer
//!
//! # Important
//! If receiving data failed part way or didn't happen at all before calling `deserialize_msg`, the
//! buffer is in an invalid state. Deserializing will then most likely fail, or worse, succeed and
//! provide old or garbage data. The same applies for the opposite direction. It's up to the user to
//! make sure the buffer is used the appropriate way.
use super::stream::Stream;
use crate::bee_serde::{BeeSerde, Deserializer, Serializer};
use crate::beemsg::header::Header;
use crate::beemsg::{Msg, MsgID};
use anyhow::{bail, Result};
use bytes::BytesMut;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;

/// Fixed length of the datagrams to send and receive via UDP.
///
/// Must match DGRAMMGR_*BUF_SIZE in `AbstractDatagramListener.h` (common) and `DatagramListener.h`
/// (client_module).
const DATAGRAM_LEN: usize = 65536;

/// Reusable buffer for serialized BeeGFS messages
///
/// See module level documentation for more information.
#[derive(Debug, Default)]
pub struct MsgBuf {
    buf: BytesMut,
    header: Box<Header>,
}

impl MsgBuf {
    /// Serializes a BeeGFS message into the buffer
    pub fn serialize_msg<M: Msg>(&mut self, msg: &M) -> Result<()> {
        self.buf.truncate(0);

        if self.buf.capacity() < Header::LEN {
            self.buf.reserve(Header::LEN);
        }

        // We need to serialize the body first since we need its total length for the header.
        // Therefore, the body part (which comes AFTER the header) is split off to be passed as a
        // separate BytesMut to the serializer.
        let mut body = self.buf.split_off(Header::LEN);

        // Catching serialization errors to ensure buffer is unsplit afterwards in all cases
        let res = (|| {
            // Serialize body
            let mut ser_body = Serializer::new(&mut body, msg.build_feature_flags());
            msg.serialize(&mut ser_body)?;

            // Create and serialize header
            let header = Header::new(ser_body.bytes_written(), M::ID, msg.build_feature_flags());
            let mut ser_header = Serializer::new(&mut self.buf, msg.build_feature_flags());
            header.serialize(&mut ser_header)?;

            *self.header = header;

            Ok(()) as Result<_>
        })();

        // Put header and body back together
        self.buf.unsplit(body);

        res
    }

    /// Deserializes the BeeGFS message present in the buffer
    ///
    /// # Panic
    /// The function will panic if the buffer has not been filled with data before (e.g. by
    /// reading from stream or receiving from a socket)
    pub fn deserialize_msg<M: Msg>(&self) -> Result<M> {
        let mut des = Deserializer::new(&self.buf[Header::LEN..], self.header.msg_feature_flags);
        M::deserialize(&mut des)
    }

    /// Reads a BeeGFS message from a stream into the buffer
    pub(super) async fn read_from_stream(&mut self, stream: &mut Stream) -> Result<()> {
        if self.buf.len() < Header::LEN {
            self.buf.resize(Header::LEN, 0);
        }

        stream.read_exact(&mut self.buf[0..Header::LEN]).await?;
        let header = Header::from_buf(&self.buf[0..Header::LEN])?;
        let msg_len = header.msg_len();

        if self.buf.len() != msg_len {
            self.buf.resize(msg_len, 0);
        }

        stream
            .read_exact(&mut self.buf[Header::LEN..msg_len])
            .await?;

        *self.header = header;

        Ok(())
    }

    /// Writes the BeeGFS message from the buffer to a stream
    ///
    /// # Panic
    /// The function will panic if the buffer has not been filled with data before (e.g. by
    /// serializing a message)
    pub(super) async fn write_to_stream(&self, stream: &mut Stream) -> Result<()> {
        stream
            .write_all(&self.buf[0..self.header.msg_len()])
            .await?;
        Ok(())
    }

    /// Receives a BeeGFS message from a UDP socket into the buffer
    pub(super) async fn recv_from_socket(&mut self, sock: &Arc<UdpSocket>) -> Result<SocketAddr> {
        if self.buf.len() != DATAGRAM_LEN {
            self.buf.resize(DATAGRAM_LEN, 0);
        }

        match sock.recv_from(&mut self.buf).await {
            Ok(n) => {
                let header = Header::from_buf(&self.buf[0..Header::LEN])?;
                self.buf.truncate(header.msg_len());
                *self.header = header;
                Ok(n.1)
            }
            Err(err) => Err(err.into()),
        }
    }

    /// Sends the BeeGFS message in the buffer to a UDP socket
    ///
    /// # Panic
    /// The function will panic if the buffer has not been filled with data before (e.g. by
    /// serializing a message)
    pub(super) async fn send_to_socket(
        &self,
        sock: &UdpSocket,
        peer_addr: &SocketAddr,
    ) -> Result<()> {
        if self.buf.len() > DATAGRAM_LEN {
            bail!(
                "Datagram to be sent to {peer_addr:?} exceeds maximum length of {DATAGRAM_LEN} \
                 bytes"
            );
        }

        sock.send_to(&self.buf, peer_addr).await?;
        Ok(())
    }

    /// The [MsgID] of the serialized BeeGFS message in the buffer
    ///
    /// # Panic
    /// The function will panic if the buffer has not been filled with data before (e.g. by
    /// reading from stream or receiving from a socket)
    pub fn msg_id(&self) -> MsgID {
        self.header.msg_id
    }
}
