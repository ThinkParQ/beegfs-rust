//! Stream communication functionality

use super::{ConnCrypto, PeerIdentity};
use crate::bee_msg::{
    Header, Msg, deserialize_header, deserialize_legacy_header, serialize, serialize_legacy,
};
use crate::bee_serde::Serializable;
use anyhow::{Result, anyhow, bail};
use beecrypt::{
    AUTH_TAG_LEN, FRAME_HDR_LEN, FrameType, Handshake, KeyPairId, MAX_HANDSHAKE_FRAME_LEN,
    MAX_PLAINTEXT_LEN, Role, Transport, peek_frame,
};
use std::fmt::Debug;
use std::io;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

const HANDSHAKE_DRAIN_TIME_LIMIT: Duration = Duration::from_secs(1);

/// A connected generic stream.
///
/// Provides functionality to communicate with the connected peer. Can support multiple
/// implementations ([TcpStream] only at the moment).
#[derive(Debug)]
pub struct Stream {
    stream: InnerStream,
    pub authenticated: bool,
    pub identity: PeerIdentity,
    peer_addr: SocketAddr,
    framing: Option<Framing>,
}

#[derive(Debug)]
#[allow(dead_code)]
enum InnerStream {
    Tcp(TcpStream),
}

struct Framing {
    transport: Transport,
    send_buf: Vec<u8>,
    recv_buf: Vec<u8>,
}

impl Debug for Framing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Framing").finish_non_exhaustive()
    }
}

impl Stream {
    /// Create the stream object from a TcpStream
    pub fn from_tcp_stream(stream: TcpStream, peer_addr: SocketAddr) -> Self {
        Self {
            stream: InnerStream::Tcp(stream),
            authenticated: false,
            identity: PeerIdentity::Legacy,
            peer_addr,
            framing: None,
        }
    }

    /// Connect to peer using TCP and obtain a [Stream] object.
    ///
    /// Times out after `time_limit`.
    pub async fn connect_tcp(peer_addr: SocketAddr, time_limit: Duration) -> Result<Self> {
        let stream = match timeout(time_limit, TcpStream::connect(peer_addr)).await {
            Ok(res) => res?,
            Err(_) => bail!("Connecting a TCP stream to {peer_addr} timed out"),
        };

        Ok(Self::from_tcp_stream(stream, peer_addr))
    }

    pub async fn handshake(
        &mut self,
        role: Role,
        crypto: &ConnCrypto,
        expected_peer_id: Option<KeyPairId>,
        time_limit: Duration,
    ) -> Result<KeyPairId> {
        if self.framing.is_some() {
            bail!("Handshake on stream to {} already completed", self.addr());
        }

        let mut hs = Handshake::new(role, crypto.mode, &crypto.keypair, expected_peer_id)?;
        let mut buf = vec![0; MAX_HANDSHAKE_FRAME_LEN];

        let res = match timeout(time_limit, self.run_handshake(&mut hs, &mut buf)).await {
            Ok(res) => res,
            Err(_) => Err(anyhow!("Handshake with {} timed out", self.addr())),
        };

        if let Err(err) = res {
            if let Err(err) = self.close_after_failed_handshake(&mut hs, &mut buf).await {
                log::debug!(
                    "Closing stream to {} after failed handshake: {err:#}",
                    self.addr()
                );
            }
            return Err(err);
        }

        let peer_id = hs.peer_id()?;
        self.framing = Some(Framing {
            transport: hs.into_transport()?,
            send_buf: vec![],
            recv_buf: vec![],
        });

        Ok(peer_id)
    }

    async fn run_handshake(&mut self, hs: &mut Handshake, buf: &mut [u8]) -> Result<()> {
        let mut done = false;
        loop {
            self.flush_handshake(hs, buf).await?;
            if done {
                return Ok(());
            }

            self.read_raw(&mut buf[..FRAME_HDR_LEN]).await?;
            let len = match peek_frame(&buf[..FRAME_HDR_LEN]) {
                Ok(info) if info.frame_type != FrameType::Message => info.len,
                _ => FRAME_HDR_LEN,
            };
            self.read_raw(&mut buf[FRAME_HDR_LEN..len]).await?;

            done = hs.step(&buf[..len])?;
        }
    }

    async fn flush_handshake(&mut self, hs: &mut Handshake, buf: &mut [u8]) -> Result<()> {
        loop {
            let len = hs.pull_frame(buf)?;
            if len == 0 {
                return Ok(());
            }
            self.write_raw(&buf[..len]).await?;
        }
    }

    async fn close_after_failed_handshake(
        &mut self,
        hs: &mut Handshake,
        buf: &mut [u8],
    ) -> Result<()> {
        timeout(
            HANDSHAKE_DRAIN_TIME_LIMIT,
            self.flush_handshake(hs, &mut *buf),
        )
        .await??;

        match &mut self.stream {
            InnerStream::Tcp(s) => {
                s.shutdown().await?;
                let _ = timeout(HANDSHAKE_DRAIN_TIME_LIMIT, async {
                    while s.read(buf).await? > 0 {}
                    Ok::<_, io::Error>(())
                })
                .await;
            }
        }

        Ok(())
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

    pub fn uses_legacy_header(&self) -> bool {
        self.framing.is_none()
    }

    pub fn serialize_msg<M: Msg + Serializable>(&self, msg: &M, buf: &mut [u8]) -> Result<usize> {
        if self.uses_legacy_header() {
            serialize_legacy(msg, buf)
        } else {
            serialize(msg, buf)
        }
    }

    /// Reads one complete BeeMsg from the stream into the provided buffer.
    ///
    /// Times out after `time_limit`.
    ///
    /// **Important**: Not cancel safe. If a timeout occurs, the stream may not be reused.
    pub async fn read_msg(&mut self, buf: &mut [u8], time_limit: Duration) -> Result<Header> {
        match timeout(time_limit, self.read_msg_inner(buf)).await {
            Ok(res) => res,
            Err(_) => Err(anyhow!("Reading from stream to {} timed out", self.addr())),
        }
    }

    /// Writes one complete serialized BeeMsg to the stream.
    ///
    /// Times out after `time_limit`.
    ///
    /// **Important**: Not cancel safe. If a timeout occurs, the stream may not be reused.
    pub async fn write_msg(&mut self, buf: &[u8], time_limit: Duration) -> Result<()> {
        match timeout(time_limit, self.write_msg_inner(buf)).await {
            Ok(res) => res,
            Err(_) => Err(anyhow!("Writing to stream to {} timed out", self.addr())),
        }
    }

    async fn read_msg_inner(&mut self, buf: &mut [u8]) -> Result<Header> {
        let Some(mut framing) = self.framing.take() else {
            self.read_raw(&mut buf[..Header::LEGACY_LEN]).await?;
            let header = deserialize_legacy_header(buf)?;
            self.read_raw(&mut buf[Header::LEGACY_LEN..header.msg_len()])
                .await?;
            return Ok(header);
        };

        let res = self.recv_frames(&mut framing, buf).await;
        self.framing = Some(framing);

        deserialize_header(&buf[..res?])
    }

    async fn recv_frames(&mut self, framing: &mut Framing, buf: &mut [u8]) -> Result<usize> {
        let mut len = 0;
        loop {
            if framing.recv_buf.len() < FRAME_HDR_LEN {
                framing.recv_buf.resize(FRAME_HDR_LEN, 0);
            }
            self.read_raw(&mut framing.recv_buf[..FRAME_HDR_LEN])
                .await?;

            let frame_len = peek_frame(&framing.recv_buf[..FRAME_HDR_LEN])?.len;
            if framing.recv_buf.len() < frame_len {
                framing.recv_buf.resize(frame_len, 0);
            }
            self.read_raw(&mut framing.recv_buf[FRAME_HDR_LEN..frame_len])
                .await?;

            let (plain_len, is_final) =
                framing.transport.open(&mut framing.recv_buf[..frame_len])?;

            let buf_len = buf.len();
            buf.get_mut(len..len + plain_len)
                .ok_or_else(|| {
                    anyhow!(
                        "Received BeeMsg doesn't fit into the provided buffer of size {buf_len}"
                    )
                })?
                .copy_from_slice(&framing.recv_buf[FRAME_HDR_LEN..FRAME_HDR_LEN + plain_len]);
            len += plain_len;

            if is_final {
                return Ok(len);
            }
        }
    }

    async fn write_msg_inner(&mut self, buf: &[u8]) -> Result<()> {
        let Some(mut framing) = self.framing.take() else {
            return self.write_raw(buf).await;
        };

        let res = async {
            let chunks = buf.len().div_ceil(MAX_PLAINTEXT_LEN).max(1);
            for i in 0..chunks {
                let chunk = &buf[(i * MAX_PLAINTEXT_LEN).min(buf.len())
                    ..((i + 1) * MAX_PLAINTEXT_LEN).min(buf.len())];
                let frame_len = FRAME_HDR_LEN + chunk.len() + AUTH_TAG_LEN;
                if framing.send_buf.len() < frame_len {
                    framing.send_buf.resize(frame_len, 0);
                }
                framing.send_buf[FRAME_HDR_LEN..FRAME_HDR_LEN + chunk.len()].copy_from_slice(chunk);

                let len =
                    framing
                        .transport
                        .seal(&mut framing.send_buf, chunk.len(), i + 1 == chunks)?;
                self.write_raw(&framing.send_buf[..len]).await?;
            }
            Ok(())
        }
        .await;

        self.framing = Some(framing);
        res
    }

    async fn read_raw(&mut self, buf: &mut [u8]) -> Result<()> {
        match &mut self.stream {
            InnerStream::Tcp(s) => {
                s.read_exact(buf).await?;
            }
        }
        Ok(())
    }

    async fn write_raw(&mut self, buf: &[u8]) -> Result<()> {
        match &mut self.stream {
            InnerStream::Tcp(s) => {
                s.write_all(buf).await?;
            }
        }
        Ok(())
    }

    /// The connected remote peers [SocketAddr]
    pub fn addr(&self) -> SocketAddr {
        self.peer_addr
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::bee_msg::deserialize_body;
    use crate::bee_msg::misc::{Ack, GenericResponse};
    use beecrypt::{KeyPair, Mode};
    use tokio::net::TcpListener;

    const TL: Duration = Duration::from_secs(5);

    fn crypto(mode: Mode) -> ConnCrypto {
        let pem = KeyPair::generate().unwrap();
        ConnCrypto::new(mode, KeyPair::load(pem.as_bytes()).unwrap())
    }

    async fn pair() -> (Stream, Stream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (client, server) = tokio::join!(Stream::connect_tcp(addr, TL), listener.accept());
        let (server, peer_addr) = server.unwrap();
        (client.unwrap(), Stream::from_tcp_stream(server, peer_addr))
    }

    async fn round_trip(mode: Mode) {
        let (mut client, mut server) = pair().await;
        let client_crypto = crypto(mode);
        let server_crypto = crypto(mode);

        let (client_res, server_res) = tokio::join!(
            client.handshake(
                Role::Initiator,
                &client_crypto,
                Some(server_crypto.key_id()),
                TL
            ),
            server.handshake(Role::Responder, &server_crypto, None, TL)
        );
        assert_eq!(client_res.unwrap(), server_crypto.key_id());
        assert_eq!(server_res.unwrap(), client_crypto.key_id());

        let msg = GenericResponse {
            code: 7,
            description: (0..MAX_PLAINTEXT_LEN + 1000)
                .map(|i| (i % 255 + 1) as u8)
                .collect(),
        };
        let mut send_buf = vec![0; 2 * MAX_PLAINTEXT_LEN];
        let mut recv_buf = vec![0; 2 * MAX_PLAINTEXT_LEN];

        let len = client.serialize_msg(&msg, &mut send_buf).unwrap();
        let (w, r) = tokio::join!(
            client.write_msg(&send_buf[..len], TL),
            server.read_msg(&mut recv_buf, TL)
        );
        w.unwrap();
        let header = r.unwrap();
        assert_eq!(header.msg_id(), GenericResponse::ID);
        assert_eq!(header.msg_len(), len);
        let recv: GenericResponse =
            deserialize_body(&header, &recv_buf[Header::LEN..header.msg_len()]).unwrap();
        assert_eq!(recv, msg);

        let resp = Ack {
            ack_id: b"response".to_vec(),
        };
        let len = server.serialize_msg(&resp, &mut send_buf).unwrap();
        server.write_msg(&send_buf[..len], TL).await.unwrap();
        let header = client.read_msg(&mut recv_buf, TL).await.unwrap();
        let recv: Ack =
            deserialize_body(&header, &recv_buf[Header::LEN..header.msg_len()]).unwrap();
        assert_eq!(recv, resp);
    }

    #[tokio::test]
    async fn round_trip_plain() {
        round_trip(Mode::Plain).await;
    }

    #[tokio::test]
    async fn round_trip_encrypted() {
        round_trip(Mode::Encrypted).await;
    }

    #[tokio::test]
    async fn peer_mismatch() {
        let (mut client, mut server) = pair().await;
        let client_crypto = crypto(Mode::Encrypted);
        let server_crypto = crypto(Mode::Encrypted);

        let (client_res, server_res) = tokio::join!(
            client.handshake(
                Role::Initiator,
                &client_crypto,
                Some(client_crypto.key_id()),
                TL
            ),
            server.handshake(Role::Responder, &server_crypto, None, TL)
        );
        client_res.unwrap_err();
        server_res.unwrap_err();
    }

    #[tokio::test]
    async fn mode_mismatch() {
        let (mut client, mut server) = pair().await;
        let client_crypto = crypto(Mode::Plain);
        let server_crypto = crypto(Mode::Encrypted);

        let (client_res, server_res) = tokio::join!(
            client.handshake(
                Role::Initiator,
                &client_crypto,
                Some(server_crypto.key_id()),
                TL
            ),
            server.handshake(Role::Responder, &server_crypto, None, TL)
        );
        client_res.unwrap_err();
        server_res.unwrap_err();
    }
}
