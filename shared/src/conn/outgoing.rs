//! Outgoing communication functionality
use super::handshake;
use super::protocol::Protocol;
use super::store::Store;
use crate::bee_msg::misc::AuthenticateChannel;
use crate::bee_msg::{Header, Msg, deserialize_body, serialize_body};
use crate::bee_serde::{Deserializable, Serializable};
use crate::conn::store::StoredStream;
use crate::conn::stream::Stream;
use crate::conn::{CONNECT_STREAM_TIME_LIMIT, GENERIC_STREAM_TIME_LIMIT, Lookup, TCP_BUF_LEN};
use crate::types::Uid;
use anyhow::{Context, Result, bail};
use std::fmt::Debug;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::timeout;

#[derive(Debug)]
pub struct PoolConfig {
    pub connection_limit: usize,
    pub protocol: Protocol,
    pub use_ipv6: bool,
}

/// The connection pool.
///
/// Provides methods for making requests to nodes (streams and datagrams / UDP). Uses [Store]
/// for storing and obtaining open streams as well as obtaining the addresses belonging to
/// a given [EntityUID].
///
/// Meant to be wrapped in an [Arc] or another sharable struct and provided to tasks for access to
/// communication.
#[derive(Debug)]
pub struct Pool<L: Lookup> {
    conn_store: Store<Uid>,
    lookup: L,
    udp_socket: Arc<UdpSocket>,
    config: PoolConfig,
}

impl<L: Lookup> Pool<L> {
    /// Creates a new Pool.
    pub fn new(lookup: L, udp_socket: Arc<UdpSocket>, config: PoolConfig) -> Self {
        Self {
            conn_store: Store::new(config.connection_limit),
            lookup,
            udp_socket,
            config,
        }
    }

    /// Send a [Msg] to a node, receive the response and return it together with its header.
    pub async fn request<M: Msg + Serializable, R: Msg + Deserializable>(
        &self,
        node_uid: Uid,
        msg: &M,
    ) -> Result<(R, Header)> {
        log::trace!("REQUEST to {node_uid:?}: {msg:?}");

        let mut buf = self.conn_store.pop_buf_or_create();

        let header = serialize_body(msg, &mut buf)?;
        let resp_header = self
            .comm_stream(node_uid, &mut buf, &header, Some(R::RESPONSE_TIME_LIMIT))
            .await?;
        let resp_msg = deserialize_body(&resp_header, &buf[Header::END_POS..])?;

        self.conn_store.push_buf(buf);

        log::trace!("RESPONSE RECEIVED from {node_uid:?}: {resp_msg:?}");

        Ok((resp_msg, resp_header))
    }

    /// Sends a [Msg] to a node and does **not** receive a response.
    pub async fn send<M: Msg + Serializable>(&self, node_uid: Uid, msg: &M) -> Result<()> {
        log::trace!("SEND to {node_uid:?}: {msg:?}");

        let mut buf = self.conn_store.pop_buf_or_create();

        let header = serialize_body(msg, &mut buf)?;
        self.comm_stream(node_uid, &mut buf, &header, None).await?;

        self.conn_store.push_buf(buf);

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
    ///
    /// If `response_time_limit` is set to `Some(t)`, a response is expected.
    async fn comm_stream(
        &self,
        node_uid: Uid,
        buf: &mut [u8],
        header: &Header,
        response_time_limit: Option<Duration>,
    ) -> Result<Header> {
        debug_assert_eq!(buf.len(), TCP_BUF_LEN);

        // 1. Pop open streams until communication succeeds or none are left
        while let Some(stream) = self.conn_store.try_pop_stream(node_uid) {
            match self
                .write_and_read_stream(buf, stream, header, response_time_limit)
                .await
            {
                Ok(header) => return Ok(header),
                Err(err) => {
                    // If the stream doesn't work anymore, just discard it and try the next one
                    log::debug!(
                        "Communication using existing stream to node with uid {node_uid} failed: {err}"
                    )
                }
            }
        }

        // 2. Obtain a permit and try to open a new stream on each available address
        if let Some(permit) = self.conn_store.try_acquire_permit(node_uid) {
            let Some(addrs) = self.lookup.node_addrs(node_uid).await? else {
                bail!("No available addresses for node with uid {node_uid}");
            };

            log::debug!("Connecting new stream to node with uid {node_uid}");

            for addr in addrs.iter() {
                if addr.is_ipv6() && !self.config.use_ipv6 {
                    continue;
                }

                match Stream::connect_tcp(addr, &self.config.protocol, CONNECT_STREAM_TIME_LIMIT)
                    .await
                {
                    Ok(stream) => {
                        let mut stream = StoredStream::from_stream(stream, permit);

                        let err_context = || {
                            format!(
                                "Connected to node with uid {node_uid}, but communication failed"
                            )
                        };

                        // Authenticate to the peer if required
                        match self.config.protocol {
                            Protocol::Legacy(Some(auth_secret)) => {
                                // The provided buffer contains the actual message to be sent later
                                // - obtain an additional one for the auth message
                                let mut auth_buf = self.conn_store.pop_buf_or_create();
                                let auth_header = serialize_body(
                                    &AuthenticateChannel { auth_secret },
                                    &mut auth_buf,
                                )?;

                                stream
                                    .as_mut()
                                    .write_msg(
                                        &mut auth_buf,
                                        &auth_header,
                                        GENERIC_STREAM_TIME_LIMIT,
                                    )
                                    .await
                                    .with_context(err_context)?;

                                self.conn_store.push_buf(auth_buf);
                            }

                            Protocol::Protected(ref p) => {
                                // If a key can't be found in the store or handshake fails, error
                                // out immediately as this can't be fixed by trying other addresses.
                                let peer_key =
                                    self.lookup.key_by_node(node_uid).await?.ok_or_else(|| {
                                        anyhow::anyhow!(
                                            "No BeeMsg public key known for node with uid \
                                            {node_uid}, cannot authenticate to it"
                                        )
                                    })?;

                                handshake::initiate(stream.as_mut(), p, peer_key)
                                    .await
                                    .with_context(err_context)?;
                            }
                            Protocol::Plain | Protocol::Legacy(None) => {}
                        }

                        // Communication using the newly opened stream should usually not fail. If
                        // it does, abort. It might be better to just try the next address though.
                        let resp_header = self
                            .write_and_read_stream(buf, stream, header, response_time_limit)
                            .await
                            .with_context(err_context)?;

                        return Ok(resp_header);
                    }
                    // If connecting failed, try the next address
                    Err(err) => log::debug!(
                        "Connecting to node with uid {node_uid} via {addr} failed: {err}"
                    ),
                }
            }

            // ... but if all failed, that's it
            bail!(
                "Connecting to node with uid {node_uid} failed for all known addresses: {addrs:?}"
            )
        }

        // 3. Wait for an already open stream becoming available. The timeout is intentionally
        // chosen short to avoid big pile up of waiting requests but rather fail quickly. The
        // user can always increase the connection limit to work around it. It's also in the
        // responsibility of requesters to limit potentially long running requests (e.g. quota
        // queries) to not block the whole pool.
        let stream = timeout(
            GENERIC_STREAM_TIME_LIMIT,
            self.conn_store.pop_stream(node_uid),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!("Popping a stream for node with uid {node_uid:?} timed out")
        })?;

        let resp_header = self
            .write_and_read_stream(buf, stream, header, response_time_limit)
            .await
            .with_context(|| {
                format!("Communication using existing stream to node with uid {node_uid} failed")
            })?;

        Ok(resp_header)
    }

    /// Writes data to the given stream, optionally receives a response and pushes the stream to
    /// the store. Receives a response if `response_time_limit` is `Some(t)`.
    async fn write_and_read_stream(
        &self,
        buf: &mut [u8],
        mut stream: StoredStream<Uid>,
        header: &Header,
        response_time_limit: Option<Duration>,
    ) -> Result<Header> {
        stream
            .as_mut()
            .write_msg(buf, header, GENERIC_STREAM_TIME_LIMIT)
            .await?;

        let resp_header = if let Some(tl) = response_time_limit {
            stream.as_mut().read_msg(buf, tl).await?
        } else {
            Header::default()
        };

        self.conn_store.push_stream(stream);
        Ok(resp_header)
    }

    /// Broadcasts a BeeMsg datagram to all given nodes using all their known addresses
    ///
    /// Logs errors if sending failed completely for a node, only fails if serialization fails.
    /// Remember that this is UDP and thus no errors only means that the sending was successful,
    /// not that the messages reached their destinations.
    pub async fn broadcast_datagram<M: Msg + Serializable>(
        &self,
        nodes_addrs: &[(Uid, Vec<SocketAddr>)],
        msg: &M,
    ) -> Result<()> {
        let mut buf = self.conn_store.pop_buf_or_create();

        // Datagrams always use the legacy protocol.
        let header = serialize_body(msg, &mut buf)?;

        header.serialize_legacy(&mut buf[..Header::END_POS])?;

        for (node_uid, addrs) in nodes_addrs {
            let mut errs = vec![];
            for addr in addrs.iter() {
                if addr.is_ipv6() && !self.config.use_ipv6 {
                    continue;
                }

                if let Err(err) = self
                    .udp_socket
                    .send_to(&buf[0..header.msg_len()], addr)
                    .await
                {
                    log::debug!(
                        "Sending datagram to node with uid {node_uid} using {addr} failed: {err}"
                    );
                    errs.push((addr, err));
                }
            }

            if errs.len() == addrs.len() {
                log::error!(
                    "Failed to send datagram to node with uid {node_uid} on all known addresses: {errs:?}"
                );
            }
        }

        self.conn_store.push_buf(buf);

        Ok(())
    }
}
