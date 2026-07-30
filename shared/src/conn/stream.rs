//! Stream communication functionality

use crate::crypto::Session;
use crate::types::StaticPubKey;
use anyhow::{Result, anyhow, bail};
use std::fmt::Debug;
use std::io;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

const TIMEOUT: Duration = Duration::from_secs(2);

/// A connected generic stream.
///
/// Provides functionality to communicate with the connected peer. Can support multiple
/// implementations ([TcpStream] only at the moment).
#[derive(Debug)]
pub struct Stream {
    stream: InnerStream,
    /// The symmetric keys established by the key exchange.
    ///
    /// [`None`] until the handshake completes, which is what makes "encrypted before a key exists"
    /// unrepresentable: the handshake messages themselves must go over the wire in the clear, and
    /// the send/receive paths key off this being [`Some`].
    session: Option<Session>,
    /// A session that has been derived but must not take effect until the next message has been
    /// sent, see [`Stream::stage_session`].
    pending_session: Option<(Session, StaticPubKey)>,
    /// The peer's long-term public key, once the handshake has proven they hold the matching
    /// private key. This is the authenticated identity of the peer.
    peer_static_pub: Option<StaticPubKey>,
    pub authenticated: bool,
    /// Per-direction message counter for the derived AES-GCM nonce (see [`crate::crypto`]).
    /// Monotonic for the connection lifetime; the two sides stay in lockstep over the in-order
    /// TCP stream. Mirrors `Channel::sendSeq` in the BeeGFS C++ codebase.
    send_seq: u64,
    /// Per-direction receive counter, the counterpart to [`Stream::send_seq`]. Mirrors
    /// `Channel::recvSeq` in the BeeGFS C++ codebase.
    recv_seq: u64,
}

#[derive(Debug)]
#[allow(dead_code)]
enum InnerStream {
    Tcp(TcpStream),
}

impl From<TcpStream> for Stream {
    fn from(stream: TcpStream) -> Self {
        Self {
            stream: InnerStream::Tcp(stream),
            session: None,
            pending_session: None,
            peer_static_pub: None,
            authenticated: false,
            send_seq: 0,
            recv_seq: 0,
        }
    }
}

impl Stream {
    /// Connect to peer using TCP and obtain a [Stream] object.
    ///
    /// Times out after [TIMEOUT].
    pub async fn connect_tcp(addr: &SocketAddr) -> Result<Self> {
        let stream = match timeout(TIMEOUT, TcpStream::connect(addr)).await {
            Ok(res) => res?,
            Err(_) => bail!("Connecting a TCP stream to {addr} timed out"),
        };

        Ok(Self {
            stream: InnerStream::Tcp(stream),
            session: None,
            pending_session: None,
            peer_static_pub: None,
            authenticated: false,
            send_seq: 0,
            recv_seq: 0,
        })
    }

    /// Installs the keys produced by a completed key exchange and marks the stream authenticated,
    /// taking effect immediately.
    ///
    /// A completed handshake *is* authentication - it proves the peer holds the private key
    /// matching the public key we have on file for it - so there is no separate authentication
    /// step. Both message counters restart at zero, because they count messages under *these*
    /// keys; the plaintext handshake messages are not counted.
    ///
    /// For the initiator, which installs after *receiving* the last plaintext message. The
    /// responder must use [`Stream::stage_session`] instead.
    pub fn install_session(&mut self, session: Session, peer_static_pub: StaticPubKey) {
        log::debug!(
            "Established encrypted session with {:?}, peer key {peer_static_pub}",
            self.addr()
        );

        self.session = Some(session);
        self.peer_static_pub = Some(peer_static_pub);
        self.authenticated = true;
        self.send_seq = 0;
        self.recv_seq = 0;
    }

    /// Stages a session to take effect *after* the next message is sent.
    ///
    /// The responder's final handshake message must still go out in the clear, but by the time it
    /// hands control back to the transport it has already derived the keys. Activating them
    /// immediately would encrypt that very message, which the initiator cannot yet decrypt.
    /// Staging expresses the actual contract: the session covers everything *after* the message
    /// that completes the handshake.
    pub fn stage_session(&mut self, session: Session, peer_static_pub: StaticPubKey) {
        self.pending_session = Some((session, peer_static_pub));
    }

    /// Activates a session staged by [`Stream::stage_session`]. No-op if nothing is staged.
    ///
    /// Called by the send path directly after a message has been written.
    pub fn activate_staged_session(&mut self) {
        if let Some((session, peer_static_pub)) = self.pending_session.take() {
            self.install_session(session, peer_static_pub);
        }
    }

    /// The peer's authenticated long-term public key, if the handshake has completed.
    ///
    /// Currently only read by tests. It is kept because this is the authenticated identity of the
    /// peer and therefore the hook for per-identity authorization ("may this identity act as meta
    /// node 3"), which needs the `keys` -> `identity_to_node` mapping that is not wired up yet.
    #[allow(dead_code)]
    pub fn peer_static_pub(&self) -> Option<StaticPubKey> {
        self.peer_static_pub
    }

    /// Encrypts a fully serialized outgoing message in place, consuming one send counter value.
    ///
    /// Does nothing while no session is established - the handshake messages themselves travel in
    /// the clear. Keeping the counter increment and the encryption together in one place is
    /// deliberate: the receiver derives its nonce from the matching counter, so a path that bumps
    /// one without the other silently desynchronizes the channel.
    pub fn encrypt_outgoing(&mut self, buf: &mut [u8]) -> Result<()> {
        if self.session.is_some() {
            let counter = self.next_send_seq();
            self.session
                .as_ref()
                .expect("session presence checked above")
                .encrypt(counter, buf)?;
        }

        Ok(())
    }

    /// Decrypts a fully read incoming message in place, consuming one receive counter value.
    ///
    /// The counterpart to [`Stream::encrypt_outgoing`]; does nothing while no session exists.
    pub fn decrypt_incoming(&mut self, buf: &mut [u8]) -> Result<()> {
        if self.session.is_some() {
            let counter = self.next_recv_seq();
            self.session
                .as_ref()
                .expect("session presence checked above")
                .decrypt(counter, buf)?;
        }

        Ok(())
    }

    /// Returns the current send counter and post-increments it.
    ///
    /// Used to derive the AES-GCM nonce for the next message sent on this stream. Mirrors
    /// `Channel::nextSendSeq()` in the BeeGFS C++ codebase.
    pub fn next_send_seq(&mut self) -> u64 {
        let counter = self.send_seq;
        self.send_seq += 1;
        counter
    }

    /// Returns the current receive counter and post-increments it.
    ///
    /// Used to derive the AES-GCM nonce for the next message received on this stream. Mirrors
    /// `Channel::nextRecvSeq()` in the BeeGFS C++ codebase.
    pub fn next_recv_seq(&mut self) -> u64 {
        let counter = self.recv_seq;
        self.recv_seq += 1;
        counter
    }

    /// Wait for the stream to become readable.
    ///
    /// This is the method to use to wait for incoming data. Waits indefinitely, no timeout.
    pub async fn readable(&self) -> Result<()> {
        match &self.stream {
            InnerStream::Tcp(s) => loop {
                s.readable().await?;

                match s.try_read(&mut [0; 0]) {
                    Ok(_) => break Ok(()),
                    Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                        continue;
                    }
                    Err(err) => break Err(anyhow!(err)),
                }
            },
        }
    }

    /// Reads from the stream into the provided buffer.
    ///
    /// The buffer will be filled completely before the future completes. Times out after
    /// [TIMEOUT].
    ///
    /// **Important**: Not cancel safe. If a timeout occurs, the stream may not be reused.
    // Clippy: Suppress false positive
    #[allow(clippy::needless_pass_by_ref_mut)]
    pub async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        match timeout(TIMEOUT, async {
            match &mut self.stream {
                InnerStream::Tcp(s) => {
                    s.read_exact(buf).await?;
                    Ok(()) as Result<_>
                }
            }
        })
        .await
        {
            Ok(res) => res,
            Err(_) => Err(anyhow!("Reading from stream to {} timed out", self.addr())),
        }
    }

    /// Writes to the stream from the provided buffer.
    ///
    /// The buffer will be written completely before the future completes. Times out after
    /// [TIMEOUT].
    ///
    /// **Important**: Not cancel safe. If a timeout occurs, the stream may not be reused.
    pub async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        match timeout(TIMEOUT, async {
            match &mut self.stream {
                InnerStream::Tcp(s) => {
                    s.write_all(buf).await?;
                    Ok(()) as Result<_>
                }
            }
        })
        .await
        {
            Ok(res) => res,
            Err(_) => Err(anyhow!("Writing to a stream to {} timed out", self.addr())),
        }
    }

    /// The connected remote peers [SocketAddr]
    pub fn addr(&self) -> SocketAddr {
        // TODO unwrap ?
        match self.stream {
            InnerStream::Tcp(ref s) => s.peer_addr().unwrap(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use tokio::net::TcpListener;

    /// The send/recv counters must start at 0 and post-increment, independently per direction.
    /// This is the lockstep contract the derived AES-GCM nonce relies on (see [`crate::crypto`]).
    #[tokio::test]
    async fn seq_counters_post_increment() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut stream = Stream::connect_tcp(&addr).await.unwrap();

        // First use of each direction returns 0, then 1, 2, ... independently.
        assert_eq!(stream.next_send_seq(), 0);
        assert_eq!(stream.next_send_seq(), 1);
        assert_eq!(stream.next_send_seq(), 2);

        assert_eq!(stream.next_recv_seq(), 0);
        assert_eq!(stream.next_recv_seq(), 1);

        // The two directions do not interfere with each other.
        assert_eq!(stream.next_send_seq(), 3);
        assert_eq!(stream.next_recv_seq(), 2);
    }
}
