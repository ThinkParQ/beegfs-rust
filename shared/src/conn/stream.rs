//! Stream communication functionality

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
            authenticated: false,
            send_seq: 0,
            recv_seq: 0,
        })
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
