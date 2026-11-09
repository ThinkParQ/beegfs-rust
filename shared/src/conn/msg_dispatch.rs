//! Facilities for dispatching TCP and UDP messages to their message handlers

use super::stream::Stream;
use super::{IdentityId, PeerIdentity};
use crate::bee_msg::{Header, Msg, deserialize_body, serialize_legacy};
use crate::bee_serde::{Deserializable, Serializable};
use crate::conn::GENERIC_STREAM_TIME_LIMIT;
use anyhow::Result;
use beecrypt::KeyPairId;
use std::fmt::Debug;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;

/// Enables an object to act as a message dispatcher and being called from the generic connection
/// pool.
pub trait DispatchRequest: Clone + Debug + Send + Sync + 'static {
    fn dispatch_request(&self, chn: impl Request) -> impl Future<Output = Result<()>> + Send;
    fn lookup_identity(
        &self,
        key_id: KeyPairId,
    ) -> impl Future<Output = Result<Option<IdentityId>>> + Send;
}

/// Defines the required functionality of the object containing the request data (e.g. message and
/// peer).
///
/// Abstracts away the underlying protocol (TCP or UDP), so the message handler doesn't need
/// to know about that.
pub trait Request: Send + Sync {
    fn respond<M: Msg + Serializable>(self, msg: &M) -> impl Future<Output = Result<()>> + Send;
    fn authenticate_connection(&mut self);
    fn peer_identity(&self) -> PeerIdentity;
    fn set_peer_identity(&mut self, identity: PeerIdentity);
    fn addr(&self) -> SocketAddr;
    fn header(&self) -> &Header;
    fn deserialize_msg<M: Msg + Deserializable>(&self) -> Result<M>;
    fn is_from_udp(&self) -> bool;
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
        let msg_len = self.stream.serialize_msg(msg, self.buf)?;
        self.stream
            .write_msg(&self.buf[0..msg_len], GENERIC_STREAM_TIME_LIMIT)
            .await
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

    fn peer_identity(&self) -> PeerIdentity {
        self.stream.identity
    }

    fn set_peer_identity(&mut self, identity: PeerIdentity) {
        log::debug!(
            "Setting identity of stream from {:?} to {identity:?}",
            self.stream.addr()
        );
        self.stream.identity = identity;
    }

    fn addr(&self) -> SocketAddr {
        self.stream.addr()
    }

    fn deserialize_msg<M: Msg + Deserializable>(&self) -> Result<M> {
        let header_len = if self.stream.uses_legacy_header() {
            Header::LEGACY_LEN
        } else {
            Header::LEN
        };
        deserialize_body(self.header, &self.buf[header_len..self.header.msg_len()])
    }

    fn header(&self) -> &Header {
        self.header
    }

    fn is_from_udp(&self) -> bool {
        false
    }
}

/// Represents a request made via a UDP datagram
#[derive(Debug)]
pub struct SocketRequest<'a> {
    pub(crate) sock: Arc<UdpSocket>,
    pub(crate) peer_addr: SocketAddr,
    pub(crate) buf: &'a mut [u8],
    pub header: &'a Header,
}

impl Request for SocketRequest<'_> {
    async fn respond<M: Msg + Serializable>(self, msg: &M) -> Result<()> {
        let msg_len = serialize_legacy(msg, self.buf)?;
        self.sock
            .send_to(&self.buf[0..msg_len], &self.peer_addr)
            .await?;
        Ok(())
    }

    fn authenticate_connection(&mut self) {
        // No authentication mechanism for sockets
    }

    fn peer_identity(&self) -> PeerIdentity {
        PeerIdentity::Legacy
    }

    fn set_peer_identity(&mut self, _identity: PeerIdentity) {}

    fn addr(&self) -> SocketAddr {
        self.peer_addr
    }

    fn deserialize_msg<M: Msg + Deserializable>(&self) -> Result<M> {
        deserialize_body(
            self.header,
            &self.buf[Header::LEGACY_LEN..self.header.msg_len()],
        )
    }

    fn header(&self) -> &Header {
        self.header
    }

    fn is_from_udp(&self) -> bool {
        true
    }
}

pub mod test {
    use super::*;
    use crate::bee_msg::Header;
    use std::net::{Ipv4Addr, SocketAddrV4};

    pub struct TestRequest {
        pub header: Header,
        pub authenticate_connection: bool,
        pub identity: PeerIdentity,
    }

    impl TestRequest {
        pub fn new(header: Header) -> Self {
            Self {
                header,
                authenticate_connection: false,
                identity: PeerIdentity::Legacy,
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

        fn peer_identity(&self) -> PeerIdentity {
            self.identity
        }

        fn set_peer_identity(&mut self, identity: PeerIdentity) {
            self.identity = identity;
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

        fn is_from_udp(&self) -> bool {
            unimplemented!()
        }
    }
}
