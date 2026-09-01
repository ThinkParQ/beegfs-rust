//! Stream communication functionality

use super::handshake::AuthenticatedPeer;
use super::noise::Transport;
use crate::bee_msg::{Header, deserialize_header, deserialize_header_v2};
use crate::protocol::{
    FRAME_HEADER_LEN, FrameHeader, FrameType, Protocol, RECORD_LEN, RECORD_PLAINTEXT_LEN,
    record_frame_len, record_plaintext_len,
};
use anyhow::{Result, anyhow, bail, ensure};
use std::fmt::Debug;
use std::io;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

/// Upper bound for a key exchange frame payload. The largest is a `Reject` with a full detail
/// string.
pub(super) const MAX_CONTROL_PAYLOAD_LEN: usize = 512;

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
///
/// Separate from [`Protocol`] because it is per stream state, not configuration: it advances as a
/// connection is set up and makes framing a message the peer cannot read unrepresentable.
#[derive(Debug, Default)]
enum Protection {
    /// Legacy protocol, no frames.
    #[default]
    Legacy,
    /// New protocol, payload in the clear.
    Frames,
    /// New protocol, payload as Noise records. Only reachable through a completed handshake, which
    /// makes protecting a message the peer cannot read unrepresentable.
    Records(Transport),
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
            peer: None,
        }
    }
}

/// Reads into the whole buffer. Free function so callers can hold a mutable borrow of another
/// [`Stream`] field at the same time, which the record loops need.
async fn read_exact_inner(
    stream: &mut InnerStream,
    buf: &mut [u8],
    time_limit: Duration,
) -> Result<()> {
    let addr = inner_addr(stream);

    match timeout(time_limit, async {
        match stream {
            InnerStream::Tcp(s) => {
                s.read_exact(buf).await?;
                Ok(()) as Result<_>
            }
        }
    })
    .await
    {
        Ok(res) => res,
        Err(_) => Err(anyhow!("Reading from stream to {addr} timed out")),
    }
}

/// Counterpart of [`read_exact_inner`].
async fn write_all_inner(stream: &mut InnerStream, buf: &[u8], time_limit: Duration) -> Result<()> {
    let addr = inner_addr(stream);

    match timeout(time_limit, async {
        match stream {
            InnerStream::Tcp(s) => {
                s.write_all(buf).await?;
                Ok(()) as Result<_>
            }
        }
    })
    .await
    {
        Ok(res) => res,
        Err(_) => Err(anyhow!("Writing to a stream to {addr} timed out")),
    }
}

fn inner_addr(stream: &InnerStream) -> SocketAddr {
    match stream {
        // TODO unwrap ?
        InnerStream::Tcp(s) => s.peer_addr().unwrap(),
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
            peer: None,
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
    ///
    /// The hook for per identity authorization ("may this identity act as meta node 3"), which is
    /// not implemented yet - hence unused.
    #[allow(dead_code)]
    pub fn peer(&self) -> Option<&AuthenticatedPeer> {
        self.peer.as_ref()
    }

    /// Writes a complete serialized BeeMsg from `buf[0..msg_len]`.
    ///
    /// Times out after `time_limit`, per write - an encrypted message spanning several records
    /// therefore has no overall bound.
    pub(super) async fn write_msg(
        &mut self,
        buf: &mut [u8],
        msg_len: usize,
        time_limit: Duration,
    ) -> Result<()> {
        let Protection::Records(transport) = &mut self.protection else {
            // `serialize` already wrote the frame header, so the unprotected protocols send the
            // buffer as is.
            return write_all_inner(&mut self.stream, &buf[0..msg_len], time_limit).await;
        };

        ensure!(
            msg_len >= Header::LEN,
            "A serialized BeeMsg is at least {} bytes, got {msg_len}",
            Header::LEN
        );
        let plaintext_len = msg_len - FRAME_HEADER_LEN;

        // `serialize` wrote the frame header for an unprotected payload - correct the length and
        // mark the payload as records.
        let frame = FrameHeader {
            ftype: FrameType::Message,
            protected: true,
            payload_len: record_frame_len(plaintext_len),
        };

        let mut first = true;
        for chunk in buf[FRAME_HEADER_LEN..msg_len].chunks(RECORD_PLAINTEXT_LEN) {
            let out = transport.seal_record(chunk)?;

            // The frame header goes into the space reserved in front of the first record, so a
            // message that fits one record leaves in a single write.
            let out = if first {
                frame.encode(out)?;
                first = false;
                &out[..]
            } else {
                &out[Transport::SEND_PREFIX_LEN..]
            };

            write_all_inner(&mut self.stream, out, time_limit).await?;
        }

        Ok(())
    }

    /// Writes a small key exchange frame.
    pub(super) async fn write_control_frame(
        &mut self,
        ftype: FrameType,
        payload: &[u8],
        time_limit: Duration,
    ) -> Result<()> {
        let mut buf = [0u8; FRAME_HEADER_LEN + MAX_CONTROL_PAYLOAD_LEN];

        let end = FRAME_HEADER_LEN + payload.len();
        let buf = buf.get_mut(..end).ok_or_else(|| {
            anyhow!(
                "A key exchange frame carries at most {MAX_CONTROL_PAYLOAD_LEN} bytes, got {}",
                payload.len()
            )
        })?;

        FrameHeader {
            ftype,
            protected: false,
            payload_len: payload.len(),
        }
        .encode(buf)?;
        buf[FRAME_HEADER_LEN..].copy_from_slice(payload);

        write_all_inner(&mut self.stream, buf, time_limit).await
    }

    /// Reads a small key exchange frame, putting the payload at the start of `buf`.
    ///
    /// # Return value
    /// Returns the frame type and the payload length.
    pub(super) async fn read_control_frame(
        &mut self,
        buf: &mut [u8],
        time_limit: Duration,
    ) -> Result<(FrameType, usize)> {
        let mut header = [0u8; FRAME_HEADER_LEN];
        read_exact_inner(&mut self.stream, &mut header, time_limit).await?;

        let frame = FrameHeader::decode(&header)?;
        ensure!(
            !frame.protected,
            "Key exchange frames are never protected, but the peer marked one as such"
        );

        ensure!(
            frame.payload_len <= buf.len(),
            "Peer announced a {} byte key exchange frame, at most {} are accepted",
            frame.payload_len,
            buf.len()
        );
        read_exact_inner(&mut self.stream, &mut buf[..frame.payload_len], time_limit).await?;

        Ok((frame.ftype, frame.payload_len))
    }

    /// Reads one complete BeeMsg into `buf`, which must be the whole message buffer so the
    /// announced length can be checked against it.
    ///
    /// `time_limit` applies to each read separately, matching how the legacy path bounded its
    /// header and body reads individually.
    ///
    /// # Return value
    /// Returns the deserialized header.
    pub(super) async fn read_msg(
        &mut self,
        buf: &mut [u8],
        time_limit: Duration,
    ) -> Result<Header> {
        if matches!(self.protection, Protection::Legacy) {
            self.read_exact(&mut buf[0..Header::LEN], time_limit)
                .await?;
            let header = deserialize_header(buf)?;

            self.read_exact(&mut buf[Header::LEN..header.msg_len()], time_limit)
                .await?;

            return Ok(header);
        }

        let mut header = [0u8; FRAME_HEADER_LEN];
        read_exact_inner(&mut self.stream, &mut header, time_limit).await?;
        let frame = FrameHeader::decode(&header)?;

        ensure!(
            frame.ftype == FrameType::Message,
            "Expected a BeeMsg frame on an established stream, got {:?}",
            frame.ftype
        );

        // Disjoint field borrows: the record loop needs the transport and the socket at once.
        let plaintext_len = match &mut self.protection {
            Protection::Records(transport) => {
                ensure!(
                    frame.protected,
                    "Peer sent an unprotected frame although encryption was negotiated"
                );

                let plaintext_len = record_plaintext_len(frame.payload_len)?;
                ensure!(
                    FRAME_HEADER_LEN + plaintext_len <= buf.len(),
                    "Received BeeMsg doesn't fit into the provided buffer: Reported plaintext {}, \
                    buffer size is {}",
                    plaintext_len,
                    buf.len()
                );

                let mut written = 0;
                let mut remaining = frame.payload_len;
                while remaining > 0 {
                    let record_len = remaining.min(RECORD_LEN);

                    read_exact_inner(
                        &mut self.stream,
                        transport.record_in(record_len)?,
                        time_limit,
                    )
                    .await?;
                    written += transport
                        .open_record(record_len, &mut buf[FRAME_HEADER_LEN + written..])?;

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
                    !frame.protected,
                    "Peer sent a protected frame although no encryption was negotiated"
                );

                // Checked before slicing - the length comes from the peer.
                let end = FRAME_HEADER_LEN + frame.payload_len;
                ensure!(
                    end <= buf.len(),
                    "Received BeeMsg doesn't fit into the provided buffer: Reported frame payload \
                    {}, buffer size is {}",
                    frame.payload_len,
                    buf.len()
                );

                read_exact_inner(
                    &mut self.stream,
                    &mut buf[FRAME_HEADER_LEN..end],
                    time_limit,
                )
                .await?;

                frame.payload_len
            }
        };

        deserialize_header_v2(&buf[FRAME_HEADER_LEN..], plaintext_len)
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

    /// The connected remote peers [SocketAddr]
    pub fn addr(&self) -> SocketAddr {
        // TODO unwrap ?
        match self.stream {
            InnerStream::Tcp(ref s) => s.peer_addr().unwrap(),
        }
    }
}
