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
        })
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
