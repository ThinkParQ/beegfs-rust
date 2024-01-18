//! Outgoing communication functionality
use super::msg_buf::MsgBuf;
use super::store::Store;
use crate::beemsg::misc::AuthenticateChannel;
use crate::beemsg::{self, Msg};
use crate::conn::store::StoredStream;
use crate::conn::stream::Stream;
use crate::types::{AuthenticationSecret, EntityUID};
use anyhow::{bail, Context, Result};
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
    auth_secret: Option<AuthenticationSecret>,
}

impl Pool {
    /// Creates a new Pool.
    pub fn new(
        udp_socket: Arc<UdpSocket>,
        connection_limit: usize,
        auth_secret: Option<AuthenticationSecret>,
    ) -> Self {
        Self {
            store: Store::new(connection_limit),
            auth_secret,
            udp_socket,
        }
    }

    /// Sends a [Msg] to a node and receives the response.
    pub async fn request<M: beemsg::Msg, R: beemsg::Msg>(
        &self,
        node_uid: EntityUID,
        msg: &M,
    ) -> Result<R> {
        log::debug!(target: "msg", "REQUEST to {:?}: {:?}", node_uid, msg);

        let mut buf = self.store.pop_buf().unwrap_or_default();

        buf.serialize_msg(msg)?;
        self.comm_stream(node_uid, &mut buf, true).await?;
        let resp = buf.deserialize_msg()?;

        self.store.push_buf(buf);

        log::debug!(target: "msg", "RESPONSE RECEIVED from {:?}: {:?}", node_uid, resp);

        Ok(resp)
    }

    /// Sends a [Msg] to a node and does **not** receive a response.
    pub async fn send<M: beemsg::Msg>(&self, node_uid: EntityUID, msg: &M) -> Result<()> {
        log::debug!(target: "msg", "SEND to {:?}: {:?}", node_uid, msg);

        let mut buf = self.store.pop_buf().unwrap_or_default();

        buf.serialize_msg(msg)?;
        self.comm_stream(node_uid, &mut buf, false).await?;

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
        node_uid: EntityUID,
        buf: &mut MsgBuf,
        expect_response: bool,
    ) -> Result<()> {
        // 1. Pop open streams until communication succeeds or none are left
        while let Some(stream) = self.store.try_pop_stream(node_uid) {
            match self
                .write_and_read_stream(buf, stream, expect_response)
                .await
            {
                Ok(_) => return Ok(()),
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
                            let mut auth_buf = self.store.pop_buf().unwrap_or_default();
                            auth_buf.serialize_msg(&AuthenticateChannel { auth_secret })?;
                            auth_buf
                                .write_to_stream(stream.as_mut())
                                .await
                                .with_context(err_context)?;
                            self.store.push_buf(auth_buf);
                        }

                        // Communication using the newly opened stream should usually not fail. If
                        // it does, abort. It might be better to just try the next address though.
                        self.write_and_read_stream(buf, stream, expect_response)
                            .await
                            .with_context(err_context)?;

                        return Ok(());
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

        self.write_and_read_stream(buf, stream, expect_response)
            .await
            .with_context(|| {
                format!("Communication using existing stream to {node_uid:?} failed")
            })?;

        Ok(())
    }

    /// Writes data to the given stream, optionally receives a response and pushes the stream to
    /// the store
    async fn write_and_read_stream(
        &self,
        buf: &mut MsgBuf,
        mut stream: StoredStream,
        expect_response: bool,
    ) -> Result<()> {
        buf.write_to_stream(stream.as_mut()).await?;

        if expect_response {
            buf.read_from_stream(stream.as_mut()).await?;
        }

        self.store.push_stream(stream);
        Ok(())
    }

    pub async fn broadcast_datagram<M: Msg>(
        &self,
        peers: impl IntoIterator<Item = EntityUID>,
        msg: &M,
    ) -> Result<()> {
        let mut buf = self.store.pop_buf().unwrap_or_default();
        buf.serialize_msg(msg)?;

        for node_uid in peers {
            let Some(addrs) = self.store.get_node_addrs(node_uid) else {
                bail!("No known addresses of {node_uid:?}");
            };

            for addr in addrs.iter() {
                buf.send_to_socket(&self.udp_socket, addr).await?;
            }
        }

        self.store.push_buf(buf);

        Ok(())
    }

    pub fn replace_node_addrs(&self, node_uid: EntityUID, new_addrs: impl Into<Arc<[SocketAddr]>>) {
        self.store.replace_node_addrs(node_uid, new_addrs)
    }
}

/// Sends a msg to a node by [SocketAddr] and receives the response.
///
/// Does not use stored connections.
pub async fn request_by_addr<M: Msg, R: Msg>(
    dest: &SocketAddr,
    msg: &M,
    auth_secret: Option<AuthenticationSecret>,
) -> Result<R> {
    let mut stream = Stream::connect_tcp(dest).await?;
    let mut buf = MsgBuf::default();

    if let Some(auth_secret) = auth_secret {
        buf.serialize_msg(&AuthenticateChannel { auth_secret })?;
        buf.write_to_stream(&mut stream).await?;
    }

    buf.serialize_msg(msg)?;
    buf.write_to_stream(&mut stream).await?;
    buf.read_from_stream(&mut stream).await?;
    let resp = buf.deserialize_msg()?;

    Ok(resp)
}

/// Sends a msg to a node by [SocketAddr] without receiving a response.
///
/// Does not use stored connections.
pub async fn send_by_addr<M: Msg>(
    dest: &SocketAddr,
    msg: &M,
    auth_secret: Option<AuthenticationSecret>,
) -> Result<()> {
    let mut stream = Stream::connect_tcp(dest).await?;
    let mut buf = MsgBuf::default();

    if let Some(auth_secret) = auth_secret {
        buf.serialize_msg(&AuthenticateChannel { auth_secret })?;
        buf.write_to_stream(&mut stream).await?;
    }

    buf.serialize_msg(msg)?;
    buf.write_to_stream(&mut stream).await?;

    Ok(())
}
