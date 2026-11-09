//! Outgoing communication functionality
use super::store::Store;
use crate::bee_msg::misc::AuthenticateChannel;
use crate::bee_msg::{Header, Msg, deserialize_body, serialize, serialize_legacy};
use crate::bee_serde::{Deserializable, Serializable};
use crate::conn::store::StoredStream;
use crate::conn::stream::Stream;
use crate::conn::{CONNECT_STREAM_TIME_LIMIT, ConnCrypto, GENERIC_STREAM_TIME_LIMIT, TCP_BUF_LEN};
use crate::types::{AuthSecret, Uid};
use anyhow::{Context, Result, anyhow, bail};
use beecrypt::{KeyPairId, Role};
use std::fmt::Debug;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::timeout;

pub trait PeerIdLookup: Debug + Send + Sync + 'static {
    fn expected_peer_id(
        &self,
        node_uid: Uid,
    ) -> Pin<Box<dyn Future<Output = Result<Option<KeyPairId>>> + Send + '_>>;
}

#[derive(Debug, Clone)]
pub struct OutgoingCrypto {
    pub crypto: ConnCrypto,
    pub peer_ids: Arc<dyn PeerIdLookup>,
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
pub struct Pool {
    store: Store<Uid>,
    udp_socket: Arc<UdpSocket>,
    auth_secret: Option<AuthSecret>,
    crypto: Option<OutgoingCrypto>,
    use_ipv6: bool,
}

impl Pool {
    /// Creates a new Pool.
    pub fn new(
        udp_socket: Arc<UdpSocket>,
        connection_limit: usize,
        auth_secret: Option<AuthSecret>,
        crypto: Option<OutgoingCrypto>,
        use_ipv6: bool,
    ) -> Self {
        Self {
            store: Store::new(connection_limit),
            auth_secret,
            crypto,
            udp_socket,
            use_ipv6,
        }
    }

    /// Send a [Msg] to a node, receive the response and return it together with its header.
    pub async fn request<M: Msg + Serializable, R: Msg + Deserializable>(
        &self,
        node_uid: Uid,
        msg: &M,
    ) -> Result<(R, Header)> {
        log::trace!("REQUEST to {node_uid:?}: {msg:?}");

        let mut buf = self.store.pop_buf_or_create();

        let msg_len = self.serialize_msg(msg, &mut buf)?;
        let resp_header = self
            .comm_stream(node_uid, &mut buf, msg_len, Some(R::RESPONSE_TIME_LIMIT))
            .await?;
        let resp_msg =
            deserialize_body(&resp_header, &buf[self.header_len()..resp_header.msg_len()])?;

        self.store.push_buf(buf);

        log::trace!("RESPONSE RECEIVED from {node_uid:?}: {resp_msg:?}");

        Ok((resp_msg, resp_header))
    }

    /// Sends a [Msg] to a node and does **not** receive a response.
    pub async fn send<M: Msg + Serializable>(&self, node_uid: Uid, msg: &M) -> Result<()> {
        log::trace!("SEND to {node_uid:?}: {msg:?}");

        let mut buf = self.store.pop_buf_or_create();

        let msg_len = self.serialize_msg(msg, &mut buf)?;
        self.comm_stream(node_uid, &mut buf, msg_len, None).await?;

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
    ///
    /// If `response_time_limit` is set to `Some(t)`, a response is expected.
    async fn comm_stream(
        &self,
        node_uid: Uid,
        buf: &mut [u8],
        send_len: usize,
        response_time_limit: Option<Duration>,
    ) -> Result<Header> {
        debug_assert_eq!(buf.len(), TCP_BUF_LEN);

        // 1. Pop open streams until communication succeeds or none are left
        while let Some(stream) = self.store.try_pop_stream(node_uid) {
            match self
                .write_and_read_stream(buf, stream, send_len, response_time_limit)
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
            let expected_peer_id = match self.crypto {
                Some(ref crypto) => Some(
                    crypto
                        .peer_ids
                        .expected_peer_id(node_uid)
                        .await?
                        .ok_or_else(|| {
                            anyhow!("No key pair ID is known for node with uid {node_uid}")
                        })?,
                ),
                None => None,
            };

            let Some(addrs) = self.store.get_node_addrs(node_uid) else {
                bail!("No available addresses for node with uid {node_uid}");
            };

            log::debug!("Connecting new stream to node with uid {node_uid}");

            for addr in addrs.iter().copied() {
                if addr.is_ipv6() && !self.use_ipv6 {
                    continue;
                }

                match Stream::connect_tcp(addr, CONNECT_STREAM_TIME_LIMIT).await {
                    Ok(stream) => {
                        let mut stream = StoredStream::from_stream(stream, permit);

                        let err_context = || {
                            format!(
                                "Connected to node with uid {node_uid}, but communication failed"
                            )
                        };

                        // Authenticate to the peer if required
                        if let Some(ref crypto) = self.crypto {
                            stream
                                .as_mut()
                                .handshake(
                                    Role::Initiator,
                                    &crypto.crypto,
                                    expected_peer_id,
                                    GENERIC_STREAM_TIME_LIMIT,
                                )
                                .await
                                .with_context(err_context)?;
                        } else if let Some(auth_secret) = self.auth_secret {
                            // The provided buffer contains the actual message to be sent later -
                            // obtain an additional one for the auth message
                            let mut auth_buf = self.store.pop_buf_or_create();
                            let msg_len = stream.as_ref().serialize_msg(
                                &AuthenticateChannel { auth_secret },
                                &mut auth_buf,
                            )?;

                            stream
                                .as_mut()
                                .write_msg(&auth_buf[0..msg_len], GENERIC_STREAM_TIME_LIMIT)
                                .await
                                .with_context(err_context)?;

                            self.store.push_buf(auth_buf);
                        }

                        // Communication using the newly opened stream should usually not fail. If
                        // it does, abort. It might be better to just try the next address though.
                        let resp_header = self
                            .write_and_read_stream(buf, stream, send_len, response_time_limit)
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
        let stream = timeout(GENERIC_STREAM_TIME_LIMIT, self.store.pop_stream(node_uid))
            .await
            .map_err(|_| {
                anyhow::anyhow!("Popping a stream for node with uid {node_uid:?} timed out")
            })?;

        let resp_header = self
            .write_and_read_stream(buf, stream, send_len, response_time_limit)
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
        send_len: usize,
        response_time_limit: Option<Duration>,
    ) -> Result<Header> {
        stream
            .as_mut()
            .write_msg(&buf[0..send_len], GENERIC_STREAM_TIME_LIMIT)
            .await?;

        let header = if let Some(tl) = response_time_limit {
            stream.as_mut().read_msg(buf, tl).await?
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

        let msg_len = serialize_legacy(msg, &mut buf)?;

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

    fn serialize_msg<M: Msg + Serializable>(&self, msg: &M, buf: &mut [u8]) -> Result<usize> {
        if self.crypto.is_some() {
            serialize(msg, buf)
        } else {
            serialize_legacy(msg, buf)
        }
    }

    fn header_len(&self) -> usize {
        if self.crypto.is_some() {
            Header::LEN
        } else {
            Header::LEGACY_LEN
        }
    }

    pub fn replace_node_addrs(&self, node_uid: Uid, new_addrs: impl Into<Arc<[SocketAddr]>>) {
        self.store.replace_node_addrs(node_uid, new_addrs)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::bee_msg::misc::{GetIdentities, GetIdentitiesResp, IdentityKey};
    use crate::conn::msg_dispatch::{DispatchRequest, Request};
    use crate::conn::{IdentityId, incoming};
    use crate::run_state;
    use beecrypt::{KeyPair, Mode};
    use std::net::Ipv4Addr;

    #[derive(Clone, Debug)]
    struct TestDispatch {
        known: KeyPairId,
    }

    impl DispatchRequest for TestDispatch {
        async fn dispatch_request(&self, req: impl Request) -> Result<()> {
            req.respond(&GetIdentitiesResp {
                keys: vec![IdentityKey {
                    key_id: b"key".to_vec(),
                    node_type: 2,
                    node_id: 7,
                }],
            })
            .await
        }

        async fn lookup_identity(&self, key_id: KeyPairId) -> Result<Option<IdentityId>> {
            Ok((key_id == self.known).then_some(1))
        }
    }

    #[derive(Debug)]
    struct FixedPeerId(KeyPairId);

    impl PeerIdLookup for FixedPeerId {
        fn expected_peer_id(
            &self,
            _node_uid: Uid,
        ) -> Pin<Box<dyn Future<Output = Result<Option<KeyPairId>>> + Send + '_>> {
            Box::pin(async move { Ok(Some(self.0)) })
        }
    }

    fn crypto() -> ConnCrypto {
        let pem = KeyPair::generate().unwrap();
        ConnCrypto::new(Mode::Encrypted, KeyPair::load(pem.as_bytes()).unwrap())
    }

    async fn request(
        client: &ConnCrypto,
        known_by_server: KeyPairId,
        expected: Option<KeyPairId>,
    ) -> Result<GetIdentitiesResp> {
        let server = crypto();
        let expected = expected.unwrap_or_else(|| server.key_id());

        let addr = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap();
        let (run_state, _control) = run_state::new();
        incoming::listen_tcp(
            addr,
            TestDispatch {
                known: known_by_server,
            },
            false,
            Some(server),
            run_state,
        )
        .await
        .unwrap();

        let udp = Arc::new(UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap());
        let pool = Pool::new(
            udp,
            2,
            None,
            Some(OutgoingCrypto {
                crypto: client.clone(),
                peer_ids: Arc::new(FixedPeerId(expected)),
            }),
            false,
        );
        pool.replace_node_addrs(1, [addr]);

        let (resp, _): (GetIdentitiesResp, Header) = pool.request(1, &GetIdentities {}).await?;
        let (again, _): (GetIdentitiesResp, Header) = pool.request(1, &GetIdentities {}).await?;
        assert_eq!(resp, again);
        Ok(resp)
    }

    #[tokio::test]
    async fn registered() {
        let client = crypto();
        let resp = request(&client, client.key_id(), None).await.unwrap();
        assert_eq!(resp.keys[0].node_id, 7);
    }

    #[tokio::test]
    async fn unregistered() {
        let client = crypto();
        request(&client, crypto().key_id(), None).await.unwrap_err();
    }

    #[tokio::test]
    async fn wrong_peer() {
        let client = crypto();
        request(&client, client.key_id(), Some(crypto().key_id()))
            .await
            .unwrap_err();
    }
}
