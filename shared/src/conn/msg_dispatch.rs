//! Facilities for dispatching TCP and UDP messages to their message handlers

use super::stream::Stream;
use crate::bee_msg::{Header, Msg, deserialize_body, serialize};
use crate::bee_serde::{Deserializable, Serializable};
use crate::crypto::Session;
use crate::types::StaticPubKey;
use anyhow::Result;
use std::fmt::Debug;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;

/// Enables an object to act as a message dispatcher and being called from the generic connection
/// pool.
pub trait DispatchRequest: Clone + Debug + Send + Sync + 'static {
    fn dispatch_request(&self, chn: impl Request) -> impl Future<Output = Result<()>> + Send;
}

/// Defines the required functionality of the object containing the request data (e.g. message and
/// peer).
///
/// Abstracts away the underlying protocol (TCP or UDP), so the message handler doesn't need
/// to know about that.
pub trait Request: Send + Sync {
    fn respond<M: Msg + Serializable>(self, msg: &M) -> impl Future<Output = Result<()>> + Send;
    fn authenticate_connection(&mut self);
    /// Installs the keys from a completed key exchange on the underlying channel.
    ///
    /// The counterpart to [`Request::authenticate_connection`] for the key exchange: message
    /// handlers cannot reach the [`Stream`] directly, so this is how the handshake handler hands
    /// the derived session down. No-op where there is no channel to key (datagrams).
    fn install_session(&mut self, session: Session, peer_static_pub: StaticPubKey);
    fn addr(&self) -> SocketAddr;
    fn header(&self) -> &Header;
    fn deserialize_msg<M: Msg + Deserializable>(&self) -> Result<M>;
}

/// Represents a request made via a TCP stream
#[derive(Debug)]
pub struct StreamRequest<'a> {
    pub(super) stream: &'a mut Stream,
    pub(super) buf: &'a mut [u8],
    pub header: &'a Header,
}

impl Request for StreamRequest<'_> {
    async fn respond<M: Msg + Serializable>(self, msg: &M) -> Result<()> {
        let msg_len = serialize(msg, self.buf)?;
        // No-op while the handshake is still in progress: those messages go out in the clear.
        self.stream.encrypt_outgoing(&mut self.buf[..msg_len])?;
        self.stream.write_all(&self.buf[0..msg_len]).await?;

        // A session staged by the key exchange handler takes effect only now, once its plaintext
        // response has actually been written.
        self.stream.activate_staged_session();

        Ok(())
    }

    fn install_session(&mut self, session: Session, peer_static_pub: StaticPubKey) {
        // Staged rather than installed: this is called from a message handler, and the response to
        // that message still has to go out unencrypted.
        self.stream.stage_session(session, peer_static_pub);
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

    fn deserialize_msg<M: Msg + Deserializable>(&self) -> Result<M> {
        deserialize_body(self.header, &self.buf[Header::LEN..])
    }

    fn header(&self) -> &Header {
        self.header
    }
}

/// Represents a request made via a UDP datagram
#[derive(Debug)]
pub struct SocketRequest<'a> {
    pub(crate) sock: Arc<UdpSocket>,
    pub(crate) peer_addr: SocketAddr,
    pub(crate) buf: &'a mut [u8],
    pub header: &'a Header,
    /// The process-wide datagram session, see [`Session::for_datagrams`].
    pub(crate) session: Arc<Session>,
}

impl Request for SocketRequest<'_> {
    async fn respond<M: Msg + Serializable>(self, msg: &M) -> Result<()> {
        let msg_len = serialize(msg, self.buf)?;
        // Datagrams are connectionless: encrypt with counter 0 (matching the C++ datagram path).
        // See `Session::for_datagrams` - this reuses one (key, nonce) pair for all datagrams.
        self.session.encrypt(0, &mut self.buf[..msg_len])?;
        self.sock
            .send_to(&self.buf[0..msg_len], &self.peer_addr)
            .await?;
        Ok(())
    }

    fn authenticate_connection(&mut self) {
        // No authentication mechanism for sockets
    }

    fn install_session(&mut self, _session: Session, _peer_static_pub: StaticPubKey) {
        // Datagrams are connectionless - there is no channel to key.
    }

    fn addr(&self) -> SocketAddr {
        self.peer_addr
    }

    fn deserialize_msg<M: Msg + Deserializable>(&self) -> Result<M> {
        deserialize_body(self.header, &self.buf[Header::LEN..])
    }

    fn header(&self) -> &Header {
        self.header
    }
}

pub mod test {
    use super::*;
    use crate::bee_msg::Header;
    use std::net::{Ipv4Addr, SocketAddrV4};

    pub struct TestRequest {
        pub header: Header,
        pub authenticate_connection: bool,
        /// The peer key of the session installed via [`Request::install_session`], if any. Lets
        /// tests assert that a handler completed the key exchange.
        pub installed_session_peer: Option<StaticPubKey>,
    }

    impl TestRequest {
        pub fn new(header: Header) -> Self {
            Self {
                header,
                authenticate_connection: false,
                installed_session_peer: None,
            }
        }
    }

    impl Request for TestRequest {
        async fn respond<M: Msg + Serializable>(self, _msg: &M) -> Result<()> {
            // do nothing
            Ok(())
        }

        fn authenticate_connection(&mut self) {
            self.authenticate_connection = true;
        }

        fn install_session(&mut self, _session: Session, peer_static_pub: StaticPubKey) {
            self.installed_session_peer = Some(peer_static_pub);
        }

        fn addr(&self) -> SocketAddr {
            SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0).into()
        }

        fn header(&self) -> &Header {
            &self.header
        }

        fn deserialize_msg<M: Msg + Deserializable>(&self) -> Result<M> {
            unimplemented!()
        }
    }
}
