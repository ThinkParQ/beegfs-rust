//! Stream communication functionality

use super::handshake::AuthenticatedPeer;
use super::noise::Transport;
use super::protocol::{
    FrameHeader, FrameType, Protocol, RECORD_LEN, RECORD_PLAINTEXT_LEN, TransportProtectionMode,
    record_frame_len, record_plaintext_len,
};
use crate::bee_msg::Header;
use crate::conn::protocol::MAX_FRAME_LEN;
use anyhow::{Context, Result, anyhow, bail, ensure};
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
    peer: Option<AuthenticatedPeer>,
}

/// How messages on a stream are framed and protected.
#[derive(Debug)]
enum Protection {
    Legacy,
    /// New protocol, payload in the clear.
    Frames,
    /// New protocol, payload as Noise records.
    Records(Transport),
}

impl Protection {
    fn from_protocol(protocol: &Protocol) -> Self {
        if protocol.is_legacy() {
            Self::Legacy
        } else {
            Self::Frames
        }
    }
}

#[derive(Debug)]
#[allow(dead_code)]
enum InnerStream {
    Tcp(TcpStream),
}

impl InnerStream {
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        match self {
            Self::Tcp(s) => {
                s.read_exact(buf).await?;
                Ok(()) as Result<_>
            }
        }
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        match self {
            Self::Tcp(s) => {
                s.write_all(buf).await?;
                Ok(()) as Result<_>
            }
        }
    }
}

impl Stream {
    /// Connect to peer using TCP and obtain a [Stream] object.
    ///
    /// Times out after `time_limit`.
    pub async fn connect_tcp(
        addr: &SocketAddr,
        protocol: &Protocol,
        time_limit: Duration,
    ) -> Result<Self> {
        let stream = match timeout(time_limit, TcpStream::connect(addr)).await {
            Ok(res) => res?,
            Err(_) => bail!("Connecting a TCP stream to {addr} timed out"),
        };

        Ok(Self {
            stream: InnerStream::Tcp(stream),
            authenticated: false,
            protection: Protection::from_protocol(protocol),
            peer: None,
        })
    }

    pub fn from_tcpstream(stream: TcpStream, protocol: &Protocol) -> Self {
        Self {
            stream: InnerStream::Tcp(stream),
            authenticated: false,
            protection: Protection::from_protocol(protocol),
            peer: None,
        }
    }

    /// Installs the negotiated record protection. Only [`super::handshake`] calls this.
    pub(super) fn install_transport(&mut self, transport: Transport) {
        self.protection = Protection::Records(transport);
    }

    /// Records the authenticated peer and marks the stream authenticated.
    pub(super) fn set_peer(&mut self, peer: AuthenticatedPeer) {
        self.authenticated = true;
        self.peer = Some(peer);
    }

    /// The identity proven by the key exchange.
    #[allow(dead_code)]
    pub fn peer(&self) -> Option<&AuthenticatedPeer> {
        self.peer.as_ref()
    }

    /// Writes a complete serialized BeeMsg from `buf[0..msg_len]`.
    ///
    /// `time_limit` bounds the whole operation, so an encrypted message spanning several records
    /// must be handed over completely within it.
    ///
    /// **Important**: Not cancel safe. On a timeout an arbitrary prefix of the message has been
    /// written, so the stream must be dropped rather than reused.
    pub(super) async fn write_msg(
        &mut self,
        buf: &mut [u8],
        header: &Header,
        time_limit: Duration,
    ) -> Result<()> {
        timeout(time_limit, async {
            let plaintext_len = header.msg_len() - FrameHeader::END_POS;

            // Serialize the complete header
            match self.protection {
                Protection::Legacy => {
                    header.serialize_legacy(&mut buf[..Header::END_POS])?;

                    self.stream.write_all(&buf[0..header.msg_len()]).await?;
                }
                Protection::Frames => {
                    let frame = FrameHeader {
                        frame_type: FrameType::Message,
                        protection: TransportProtectionMode::Plain,
                        frame_len: header.msg_len(),
                    };
                    frame.serialize(buf)?;
                    header.serialize(&mut buf[FrameHeader::END_POS..Header::END_POS])?;

                    self.stream.write_all(&buf[0..header.msg_len()]).await?;
                }
                Protection::Records(ref mut transport) => {
                    let frame = FrameHeader {
                        frame_type: FrameType::Message,
                        protection: TransportProtectionMode::Encrypted,
                        frame_len: record_frame_len(plaintext_len),
                    };
                    // Serializing the frame header happens below, directly into the front of the
                    // scratch buffer after encryption
                    header.serialize(&mut buf[FrameHeader::END_POS..Header::END_POS])?;

                    let mut first = true;
                    for chunk in
                        buf[FrameHeader::END_POS..header.msg_len()].chunks(RECORD_PLAINTEXT_LEN)
                    {
                        let out = transport.seal_record(chunk)?;

                        // The frame header goes into the space reserved in front of the first
                        // record, so a message that fits one record leaves
                        // in a single write.
                        let out = if first {
                            frame.serialize(out)?;
                            first = false;
                            &out[..]
                        } else {
                            &out[Transport::SEND_PREFIX_LEN..]
                        };

                        self.stream.write_all(out).await?;
                    }
                }
            }

            Ok(())
        })
        .await
        .with_context(|| {
            format!(
                "Writing message to a stream connected to {} timed out",
                self.addr()
            )
        })?
    }

    /// Reads one complete BeeMsg into `buf`, which must be the whole message buffer so the
    /// announced length can be checked against it.
    ///
    /// `time_limit` bounds the whole operation, including the peer's think time before the first
    /// byte arrives.
    ///
    /// **Important**: Not cancel safe. On a timeout, the stream must be dropped rather than reused.
    pub(super) async fn read_msg(
        &mut self,
        buf: &mut [u8],
        time_limit: Duration,
    ) -> Result<Header> {
        timeout(time_limit, async {
            if matches!(self.protection, Protection::Legacy) {
                self.stream.read_exact(&mut buf[0..Header::END_POS]).await?;
                let header = Header::deserialize_legacy(buf)?;

                self.stream
                    .read_exact(&mut buf[Header::END_POS..header.msg_len()])
                    .await?;

                return Ok(header);
            }

            let mut header = [0u8; FrameHeader::END_POS];
            self.stream.read_exact(&mut header).await?;
            let frame = FrameHeader::deserialize(&header)?;

            ensure!(
                frame.frame_type == FrameType::Message,
                "Expected a BeeMsg frame on an established stream, got {:?}",
                frame.frame_type
            );

            let plaintext_len = match self.protection {
                Protection::Records(ref mut transport) => {
                    ensure!(
                        frame.protection == TransportProtectionMode::Encrypted,
                        "Peer sent an unprotected frame although encryption was negotiated"
                    );
                    ensure!(frame.frame_len <= MAX_FRAME_LEN);

                    let plaintext_len = record_plaintext_len(frame.frame_len)?;
                    ensure!(
                        FrameHeader::END_POS + plaintext_len <= buf.len(),
                        "Received BeeMsg doesn't fit into the provided buffer: \
                        Reported plaintext {}, buffer size is {}",
                        plaintext_len,
                        buf.len()
                    );

                    let mut written = 0;
                    let mut remaining = frame.body_len();
                    while remaining > 0 {
                        let record_len = remaining.min(RECORD_LEN);

                        self.stream
                            .read_exact(transport.record_in(record_len)?)
                            .await?;
                        written += transport
                            .open_record(record_len, &mut buf[FrameHeader::END_POS + written..])?;

                        remaining -= record_len;
                    }

                    ensure!(
                        written == plaintext_len,
                        "Records decrypted to {written} bytes, expected {plaintext_len}"
                    );

                    plaintext_len
                }
                _ => {
                    ensure!(
                        frame.protection == TransportProtectionMode::Plain,
                        "Peer sent a protected frame although no encryption was negotiated"
                    );

                    // Checked before slicing - the length comes from the peer.
                    ensure!(
                        frame.frame_len <= buf.len(),
                        "Received BeeMsg doesn't fit into the provided buffer: \
                        Reported frame length {}, buffer size is {}",
                        frame.frame_len,
                        buf.len()
                    );

                    self.stream
                        .read_exact(&mut buf[FrameHeader::END_POS..frame.frame_len])
                        .await?;

                    frame.body_len()
                }
            };

            Header::deserialize(
                &buf[FrameHeader::END_POS..Header::END_POS],
                FrameHeader::END_POS + plaintext_len,
            )
        })
        .await
        .with_context(|| {
            format!(
                "Reading message from a stream connected to {} timed out",
                self.addr()
            )
        })?
    }

    pub(super) async fn write_control_frame(
        &mut self,
        frame_type: FrameType,
        buf: &mut [u8],
        time_limit: Duration,
    ) -> Result<()> {
        ensure!(buf.len() >= FrameHeader::END_POS);

        timeout(time_limit, async {
            FrameHeader {
                frame_type,
                protection: TransportProtectionMode::Plain,
                frame_len: buf.len(),
            }
            .serialize(&mut buf[..FrameHeader::END_POS])?;

            self.stream.write_all(buf).await
        })
        .await
        .with_context(|| {
            format!(
                "Writing control frame to a stream connected to {} timed out",
                self.addr()
            )
        })?
    }

    pub(super) async fn read_control_frame(
        &mut self,
        buf: &mut [u8],
        time_limit: Duration,
    ) -> Result<FrameType> {
        ensure!(buf.len() >= FrameHeader::END_POS);

        timeout(time_limit, async {
            self.stream
                .read_exact(&mut buf[..FrameHeader::END_POS])
                .await?;

            let frame = FrameHeader::deserialize(&buf[..FrameHeader::END_POS])?;
            ensure!(
                frame.protection == TransportProtectionMode::Plain,
                "Control frames are never protected, but the peer marked one as such"
            );

            ensure!(frame.frame_len <= buf.len());

            self.stream
                .read_exact(&mut buf[FrameHeader::END_POS..frame.frame_len])
                .await?;

            Ok(frame.frame_type)
        })
        .await
        .with_context(|| {
            format!(
                "Reading control frame from a stream connected to {} timed out",
                self.addr()
            )
        })?
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

    /// The connected remote peers [SocketAddr]
    pub fn addr(&self) -> SocketAddr {
        // TODO unwrap ?
        match self.stream {
            InnerStream::Tcp(ref s) => s.peer_addr().unwrap(),
        }
    }
}
