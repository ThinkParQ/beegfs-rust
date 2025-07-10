//! Outgoing communication functionality
use super::store::Store;
use crate::bee_msg::misc::AuthenticateChannel;
use crate::bee_msg::{Header, Msg, deserialize_body, deserialize_header, serialize};
use crate::bee_serde::{Deserializable, Serializable};
use crate::conn::store::StoredStream;
use crate::conn::stream::Stream;
use crate::types::{AuthSecret, Uid};
use anyhow::{Context, Result, bail};
use std::fmt::Debug;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;

/// The connection pool.
///
/// Provides methods for making requests to nodes (streams and datagrams / UDP). Uses [Store]
/// for storing and obtaining open streams as well as obtaining the addresses belonging to
/// a given [EntityUID].
///
/// Meant to be wrapped in an [Arc] or another sharable struct and provided to tasks for access to
/// communication.
#[derive(Debug)]
pub struct Pool {
    store: Store,
    udp_socket: Arc<UdpSocket>,
    auth_secret: Option<AuthSecret>,
}

impl Pool {
    /// Creates a new Pool.
    pub fn new(
        udp_socket: Arc<UdpSocket>,
        connection_limit: usize,
        auth_secret: Option<AuthSecret>,
    ) -> Self {
        Self {
            store: Store::new(connection_limit),
            auth_secret,
            udp_socket,
        }
    }

    /// Sends a [Msg] to a node and receives the response.
    pub async fn request<M: Msg + Serializable, R: Msg + Deserializable>(
        &self,
        node_uid: Uid,
        msg: &M,
    ) -> Result<R> {
        log::trace!("REQUEST to {:?}: {:?}", node_uid, msg);

        let mut buf = self.store.pop_buf_or_create();

        let msg_len = serialize(msg, &mut buf)?;
        let resp_header = self
            .comm_stream(node_uid, &mut buf[0..msg_len], true)
            .await?;
        let resp_msg = deserialize_body(&resp_header, &buf[Header::LEN..])?;

        self.store.push_buf(buf);

        log::trace!("RESPONSE RECEIVED from {:?}: {:?}", node_uid, resp_msg);

        Ok(resp_msg)
    }

    /// Sends a [Msg] to a node and does **not** receive a response.
    pub async fn send<M: Msg + Serializable>(&self, node_uid: Uid, msg: &M) -> Result<()> {
        log::trace!("SEND to {:?}: {:?}", node_uid, msg);

        let mut buf = self.store.pop_buf_or_create();

        let msg_len = serialize(msg, &mut buf)?;
        self.comm_stream(node_uid, &mut buf[0..msg_len], false)
            .await?;

        self.store.push_buf(buf);

        Ok(())
    }

    /// Write and read the buffers content using a stream.
    ///
    /// This method acquires a stream to the given node, writes the message in the buffer to it and
    /// optionally reads the response into the same buffer. When done, the stream is pushed into the
    /// store.
    ///
    /// Acquisition happens in the following order:
    ///
    /// 1. Pop open streams from the store without waiting
    /// 2. Get a permit that allows opening a new stream. Try to open a new stream using the
    ///    available addresses.
    /// 3. Pop an open stream from the store, waiting until one gets available.
    async fn comm_stream(
        &self,
        node_uid: Uid,
        buf: &mut [u8],
        expect_response: bool,
    ) -> Result<Header> {
        // 1. Pop open streams until communication succeeds or none are left
        while let Some(stream) = self.store.try_pop_stream(node_uid) {
            match self
                .write_and_read_stream(buf, stream, expect_response)
                .await
            {
                Ok(header) => return Ok(header),
                Err(err) => {
                    // If the stream doesn't work anymore, just discard it and try the next one
                    log::debug!("Communication using existing stream to {node_uid:?} failed: {err}")
                }
            }
        }

        // 2. Obtain a permit and try to open a new stream on each available address
        if let Some(permit) = self.store.try_acquire_permit(node_uid) {
            let Some(addrs) = self.store.get_node_addrs(node_uid) else {
                bail!("No available addresses to {node_uid:?}");
            };

            log::debug!("Connecting new stream to {node_uid:?}");

            for addr in addrs.iter() {
                match Stream::connect_tcp(addr).await {
                    Ok(stream) => {
                        let mut stream = StoredStream::from_stream(stream, permit);

                        let err_context =
                            || format!("Connected to {node_uid:?}, but communication failed");

                        // Authenticate to the peer if required
                        if let Some(auth_secret) = self.auth_secret {
                            // The provided buffer contains the actual message to be sent later -
                            // obtain an additional one for the auth message
                            let mut auth_buf = self.store.pop_buf_or_create();
                            let msg_len =
                                serialize(&AuthenticateChannel { auth_secret }, &mut auth_buf)?;

                            stream
                                .as_mut()
                                .write_all(&buf[0..msg_len])
                                .await
                                .with_context(err_context)?;

                            self.store.push_buf(auth_buf);
                        }

                        // Communication using the newly opened stream should usually not fail. If
                        // it does, abort. It might be better to just try the next address though.
                        let resp_header = self
                            .write_and_read_stream(buf, stream, expect_response)
                            .await
                            .with_context(err_context)?;

                        return Ok(resp_header);
                    }
                    // If connecting failed, try the next address
                    Err(err) => log::debug!("Connecting to {node_uid:?} via {addr} failed: {err}"),
                }
            }

            // ... but if all failed, that's it
            bail!("Connecting to {node_uid:?} failed for all known addresses: {addrs:?}")
        }

        // 3. Wait for an already open stream becoming available
        let stream = self.store.pop_stream(node_uid).await?;

        let resp_header = self
            .write_and_read_stream(buf, stream, expect_response)
            .await
            .with_context(|| {
                format!("Communication using existing stream to {node_uid:?} failed")
            })?;

        Ok(resp_header)
    }

    /// Writes data to the given stream, optionally receives a response and pushes the stream to
    /// the store
    async fn write_and_read_stream(
        &self,
        buf: &mut [u8],
        mut stream: StoredStream,
        expect_response: bool,
    ) -> Result<Header> {
        stream.as_mut().write_all(buf).await?;

        let header = if expect_response {
            // Read header
            stream.as_mut().read_exact(&mut buf[0..Header::LEN]).await?;
            let header = deserialize_header(&buf[0..Header::LEN])?;

            // Read body
            stream
                .as_mut()
                .read_exact(&mut buf[Header::LEN..header.msg_len()])
                .await?;
            header
        } else {
            Header::default()
        };

        self.store.push_stream(stream);
        Ok(header)
    }

    pub async fn broadcast_datagram<M: Msg + Serializable>(
        &self,
        peers: impl IntoIterator<Item = Uid>,
        msg: &M,
    ) -> Result<()> {
        let mut buf = self.store.pop_buf_or_create();

        let msg_len = serialize(msg, &mut buf)?;

        for node_uid in peers {
            let Some(addrs) = self.store.get_node_addrs(node_uid) else {
                bail!("No network address found for node with uid {node_uid:?}");
            };

            for addr in addrs.iter() {
                self.udp_socket.send_to(&buf[0..msg_len], addr).await?;
            }
        }

        self.store.push_buf(buf);

        Ok(())
    }

    pub fn replace_node_addrs(&self, node_uid: Uid, new_addrs: impl Into<Arc<[SocketAddr]>>) {
        self.store.replace_node_addrs(node_uid, new_addrs)
    }
}
