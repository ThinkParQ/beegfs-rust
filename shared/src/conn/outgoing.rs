//! Outgoing communication functionality
use super::store::Store;
use crate::bee_msg::misc::{GenericResponse, KeyExchangeRequest, KeyExchangeResponse};
use crate::bee_msg::{
    Header, Msg, deserialize_body, deserialize_encryption_header, deserialize_header, serialize,
};
use crate::bee_serde::{Deserializable, Serializable};
use crate::conn::TCP_BUF_LEN;
use crate::conn::key_store::KeyStore;
use crate::conn::store::StoredStream;
use crate::conn::stream::Stream;
use crate::crypto::Session;
use crate::crypto::handshake::{self, StaticKeypair};
use crate::types::{AuthSecret, StaticPubKey, Uid};
use anyhow::{Context, Result, bail};
use std::fmt::Debug;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::timeout;

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
    store: Store<Uid>,
    udp_socket: Arc<UdpSocket>,
    use_ipv6: bool,
    /// Our long-term identity, used as initiator in the key exchange. [`None`] disables
    /// authentication and encryption on outgoing streams entirely.
    keypair: Option<Arc<StaticKeypair>>,
    /// The public keys we expect from the nodes we dial.
    key_store: Arc<KeyStore>,
    /// Shared key for the connectionless datagram path, see [`Session::for_datagrams`].
    datagram_session: Arc<Session>,
}

impl Pool {
    /// Creates a new Pool.
    ///
    /// `auth_secret` is only used to derive the shared datagram key; stream authentication comes
    /// from `keypair` plus `key_store` via the key exchange.
    pub fn new(
        udp_socket: Arc<UdpSocket>,
        connection_limit: usize,
        auth_secret: Option<AuthSecret>,
        use_ipv6: bool,
        keypair: Option<Arc<StaticKeypair>>,
        key_store: Arc<KeyStore>,
    ) -> Self {
        Self {
            store: Store::new(connection_limit),
            udp_socket,
            use_ipv6,
            keypair,
            key_store,
            datagram_session: Arc::new(Session::for_datagrams(auth_secret)),
        }
    }

    /// The datagram session, to be shared with the receive path.
    pub fn datagram_session(&self) -> Arc<Session> {
        self.datagram_session.clone()
    }

    /// The list of peer public keys, shared with the incoming side.
    pub fn key_store(&self) -> &KeyStore {
        &self.key_store
    }

    /// Sends a [Msg] to a node and receives the response.
    pub async fn request<M: Msg + Serializable, R: Msg + Deserializable>(
        &self,
        node_uid: Uid,
        msg: &M,
    ) -> Result<R> {
        log::trace!("REQUEST to {node_uid:?}: {msg:?}");

        let mut buf = self.store.pop_buf_or_create();

        let msg_len = serialize(msg, &mut buf)?;
        let resp_header = self.comm_stream(node_uid, &mut buf, msg_len, true).await?;
        let resp_msg = deserialize_body(&resp_header, &buf[Header::LEN..])?;

        self.store.push_buf(buf);

        log::trace!("RESPONSE RECEIVED from {node_uid:?}: {resp_msg:?}");

        Ok(resp_msg)
    }

    /// Sends a [Msg] to a node and does **not** receive a response.
    pub async fn send<M: Msg + Serializable>(&self, node_uid: Uid, msg: &M) -> Result<()> {
        log::trace!("SEND to {node_uid:?}: {msg:?}");

        let mut buf = self.store.pop_buf_or_create();

        let msg_len = serialize(msg, &mut buf)?;
        self.comm_stream(node_uid, &mut buf, msg_len, false).await?;

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
        send_len: usize,
        expect_response: bool,
    ) -> Result<Header> {
        debug_assert_eq!(buf.len(), TCP_BUF_LEN);

        // 1. Pop open streams until communication succeeds or none are left
        while let Some(stream) = self.store.try_pop_stream(node_uid) {
            match self
                .write_and_read_stream(buf, stream, send_len, expect_response)
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
        if let Some(permit) = self.store.try_acquire_permit(node_uid) {
            let Some(addrs) = self.store.get_node_addrs(node_uid) else {
                bail!("No available addresses for node with uid {node_uid}");
            };

            log::debug!("Connecting new stream to node with uid {node_uid}");

            for addr in addrs.iter() {
                if addr.is_ipv6() && !self.use_ipv6 {
                    continue;
                }

                match Stream::connect_tcp(addr).await {
                    Ok(stream) => {
                        let mut stream = StoredStream::from_stream(stream, permit);

                        let err_context = || {
                            format!(
                                "Connected to node with uid {node_uid}, but communication failed"
                            )
                        };

                        // Establish the session before anything else goes over this stream. Every
                        // message after the handshake is encrypted under the derived keys.
                        if let Some(keypair) = &self.keypair {
                            let Some(peer_static) = self.key_store.key_by_node(node_uid) else {
                                bail!(
                                    "No public key known for node with uid {node_uid} - it must be \
                                     registered in the management key list before an encrypted \
                                     connection can be established"
                                );
                            };

                            // The provided buffer holds the actual message to be sent later -
                            // obtain an additional one for the handshake.
                            let mut hs_buf = self.store.pop_buf_or_create();
                            let res =
                                key_exchange(stream.as_mut(), &mut hs_buf, keypair, peer_static)
                                    .await;
                            self.store.push_buf(hs_buf);

                            res.with_context(|| {
                                format!("Key exchange with node with uid {node_uid} failed")
                            })?;
                        }

                        // Communication using the newly opened stream should usually not fail. If
                        // it does, abort. It might be better to just try the next address though.
                        let resp_header = self
                            .write_and_read_stream(buf, stream, send_len, expect_response)
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

        // 3. Wait for an already open stream becoming available
        let stream = timeout(Duration::from_secs(2), self.store.pop_stream(node_uid))
            .await
            .map_err(|_| {
                anyhow::anyhow!("Popping a stream for node with uid {node_uid:?} timed out")
            })?;

        let resp_header = self
            .write_and_read_stream(buf, stream, send_len, expect_response)
            .await
            .with_context(|| {
                format!("Communication using existing stream to node with uid {node_uid} failed")
            })?;

        Ok(resp_header)
    }

    /// Writes data to the given stream, optionally receives a response and pushes the stream to
    /// the store
    async fn write_and_read_stream(
        &self,
        buf: &mut [u8],
        mut stream: StoredStream<Uid>,
        send_len: usize,
        expect_response: bool,
    ) -> Result<Header> {
        // Encrypt the request with this stream's send counter right before sending it.
        stream.as_mut().encrypt_outgoing(&mut buf[..send_len])?;

        stream.as_mut().write_all(&buf[0..send_len]).await?;

        let header = if expect_response {
            // Read header
            stream.as_mut().read_exact(&mut buf[0..Header::LEN]).await?;
            let len = deserialize_encryption_header(&buf[0..Header::ENCRYPTION_INFO_LEN])?;

            // Read body
            stream
                .as_mut()
                .read_exact(&mut buf[Header::LEN..len])
                .await?;

            // Decrypt the response with this stream's receive counter.
            stream.as_mut().decrypt_incoming(&mut buf[..len])?;
            deserialize_header(&buf[..Header::LEN])?
        } else {
            Header::default()
        };

        self.store.push_stream(stream);
        Ok(header)
    }

    /// Broadcasts a BeeMsg datagram to all given nodes using all their known addresses
    ///
    /// Logs errors if sending failed completely for a node, only fails if serialization fails.
    /// Remember that this is UDP and thus no errors only means that the sending was successful,
    /// not that the messages reached their destinations.
    pub async fn broadcast_datagram<M: Msg + Serializable>(
        &self,
        peers: impl IntoIterator<Item = Uid>,
        msg: &M,
    ) -> Result<()> {
        let mut buf = self.store.pop_buf_or_create();

        let msg_len = serialize(msg, &mut buf)?;

        // Datagrams are connectionless, so there is no per-connection counter: encrypt once with
        // counter 0 (matching the C++ datagram path) and reuse the buffer for every peer. See
        // `Session::for_datagrams` - this means all datagrams share one (key, nonce) pair.
        self.datagram_session.encrypt(0, &mut buf[..msg_len])?;

        for node_uid in peers {
            let addrs = self.store.get_node_addrs(node_uid).unwrap_or_default();

            if addrs.is_empty() {
                log::error!(
                    "Failed to send datagram to node with uid {node_uid}: No known addresses"
                );
                continue;
            }

            let mut errs = vec![];
            for addr in addrs.iter() {
                if addr.is_ipv6() && !self.use_ipv6 {
                    continue;
                }

                if let Err(err) = self.udp_socket.send_to(&buf[0..msg_len], addr).await {
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

        self.store.push_buf(buf);

        Ok(())
    }

    pub fn replace_node_addrs(&self, node_uid: Uid, new_addrs: impl Into<Arc<[SocketAddr]>>) {
        self.store.replace_node_addrs(node_uid, new_addrs)
    }
}

/// Runs the key exchange as initiator on a freshly connected stream and installs the session.
///
/// Both handshake messages travel in the clear - they carry only public keys - which is why this
/// writes and reads the stream directly instead of going through the encrypting helpers. On
/// success the stream is encrypted and authenticated for the rest of its life.
async fn key_exchange(
    stream: &mut Stream,
    buf: &mut [u8],
    local: &StaticKeypair,
    peer_static: StaticPubKey,
) -> Result<()> {
    let initiator = handshake::Initiator::start(local, peer_static)?;

    let msg_len = serialize(
        &KeyExchangeRequest {
            version: handshake::HANDSHAKE_VERSION,
            static_pub: local.public(),
            ephemeral_pub: initiator.ephemeral_pub(),
        },
        buf,
    )?;
    stream.write_all(&buf[0..msg_len]).await?;

    // Read the reply. Framing is the same as for any BeeMsg, it is just not encrypted.
    stream.read_exact(&mut buf[0..Header::LEN]).await?;
    let resp_len = deserialize_encryption_header(&buf[0..Header::ENCRYPTION_INFO_LEN])?;
    stream.read_exact(&mut buf[Header::LEN..resp_len]).await?;

    let header = deserialize_header(&buf[0..Header::LEN])?;

    // A peer that does not accept our key answers with a GenericResponse. Surface its description
    // rather than a confusing "unexpected message id".
    if header.msg_id() == GenericResponse::ID {
        let resp: GenericResponse = deserialize_body(&header, &buf[Header::LEN..])?;
        bail!(
            "Peer rejected the key exchange: {}",
            String::from_utf8_lossy(&resp.description)
        );
    }

    if header.msg_id() != KeyExchangeResponse::ID {
        bail!(
            "Expected a key exchange response (msg id {}), got msg id {}",
            KeyExchangeResponse::ID,
            header.msg_id()
        );
    }

    let resp: KeyExchangeResponse = deserialize_body(&header, &buf[Header::LEN..])?;

    // A typed message handler cannot answer with a GenericResponse, so a peer that refuses us
    // replies with a default initialized - all zero - response. Recognise that and name the likely
    // cause, rather than letting the all-zero ephemeral key surface as a low order point error.
    if resp.ephemeral_pub == [0u8; 32] {
        bail!(
            "Peer rejected the key exchange. Our public key {} is most likely not registered in \
             the management key list - check the peer's log for the exact reason",
            local.public()
        );
    }

    let session = initiator.finish(&resp.ephemeral_pub, &resp.confirm)?;

    stream.install_session(session, peer_static);

    Ok(())
}
