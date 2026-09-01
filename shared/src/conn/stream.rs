//! Stream communication functionality

use crate::bee_msg::{Header, deserialize_header, deserialize_header_v2};
use crate::protocol::{FRAME_HEADER_LEN, FrameHeader, FrameType, Protocol};
use anyhow::{Result, anyhow, bail, ensure};
use std::fmt::Debug;
use std::io;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

/// A connected generic stream.
///
/// Provides functionality to communicate with the connected peer. Can support multiple
/// implementations ([TcpStream] only at the moment).
#[derive(Debug)]
pub struct Stream {
    stream: InnerStream,
    pub authenticated: bool,
    protection: Protection,
}

/// How messages on a stream are framed and protected.
///
/// Separate from [`Protocol`] because it is per stream state, not configuration: it advances as a
/// connection is set up and makes framing a message the peer cannot read unrepresentable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Protection {
    /// Legacy protocol, no frames.
    #[default]
    Legacy,
    /// New protocol, payload in the clear.
    Frames,
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
            protection: Protection::default(),
        }
    }
}

impl Stream {
    /// Connect to peer using TCP and obtain a [Stream] object.
    ///
    /// Times out after `time_limit`.
    pub async fn connect_tcp(addr: &SocketAddr, time_limit: Duration) -> Result<Self> {
        let stream = match timeout(time_limit, TcpStream::connect(addr)).await {
            Ok(res) => res?,
            Err(_) => bail!("Connecting a TCP stream to {addr} timed out"),
        };

        Ok(Self {
            stream: InnerStream::Tcp(stream),
            authenticated: false,
            protection: Protection::default(),
        })
    }

    /// Selects the framing for this stream. Must be called before the first message.
    pub(super) fn set_protocol(&mut self, protocol: Protocol) {
        self.protection = if protocol.is_legacy() {
            Protection::Legacy
        } else {
            Protection::Frames
        };
    }

    /// Writes a complete serialized BeeMsg from `buf[0..msg_len]`.
    ///
    /// Times out after `time_limit`.
    pub(super) async fn write_msg(
        &mut self,
        buf: &mut [u8],
        msg_len: usize,
        time_limit: Duration,
    ) -> Result<()> {
        // `serialize` already wrote the frame header, so both protocols send the buffer as is.
        self.write_all(&buf[0..msg_len], time_limit).await
    }

    /// Reads one complete BeeMsg into `buf`, which must be the whole message buffer so the
    /// announced length can be checked against it.
    ///
    /// `time_limit` applies to each read separately, matching how the legacy path bounded its
    /// header and body reads individually.
    ///
    /// # Return value
    /// Returns the deserialized header.
    pub(super) async fn read_msg(&mut self, buf: &mut [u8], time_limit: Duration) -> Result<Header> {
        if self.protection == Protection::Legacy {
            self.read_exact(&mut buf[0..Header::LEN], time_limit).await?;
            let header = deserialize_header(buf)?;

            self.read_exact(&mut buf[Header::LEN..header.msg_len()], time_limit)
                .await?;

            return Ok(header);
        }

        self.read_exact(&mut buf[0..FRAME_HEADER_LEN], time_limit)
            .await?;
        let frame = FrameHeader::decode(buf)?;

        ensure!(
            frame.ftype == FrameType::Message,
            "Expected a BeeMsg frame on an established stream, got {:?}",
            frame.ftype
        );
        ensure!(
            !frame.protected,
            "Peer sent a protected frame although no encryption was negotiated"
        );

        // Checked before slicing - the length comes from the peer.
        let end = FRAME_HEADER_LEN + frame.payload_len;
        ensure!(
            end <= buf.len(),
            "Received BeeMsg doesn't fit into the provided buffer: Reported frame payload {}, \
            buffer size is {}",
            frame.payload_len,
            buf.len()
        );

        self.read_exact(&mut buf[FRAME_HEADER_LEN..end], time_limit)
            .await?;

        deserialize_header_v2(&buf[FRAME_HEADER_LEN..], frame.payload_len)
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
    /// `time_limit`.
    ///
    /// **Important**: Not cancel safe. If a timeout occurs, the stream may not be reused.
    pub async fn read_exact(&mut self, buf: &mut [u8], time_limit: Duration) -> Result<()> {
        match timeout(time_limit, async {
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
    /// `time_limit`.
    ///
    /// **Important**: Not cancel safe. If a timeout occurs, the stream may not be reused.
    pub async fn write_all(&mut self, buf: &[u8], time_limit: Duration) -> Result<()> {
        match timeout(time_limit, async {
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
            Err(_) => Err(anyhow!("Writing to stream to {} timed out", self.addr())),
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
