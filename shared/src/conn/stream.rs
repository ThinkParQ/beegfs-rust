//! Stream communication functionality

use super::{ConnCrypto, PeerIdentity};
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
    recv_pos: usize,
    recv_end: usize,
}

impl Debug for Framing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Framing")
            .field("recv_pending", &(self.recv_end - self.recv_pos))
            .finish_non_exhaustive()
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
            recv_pos: 0,
            recv_end: 0,
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
        if self
            .framing
            .as_ref()
            .is_some_and(|f| f.recv_pos < f.recv_end)
        {
            return Ok(());
        }

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
        match timeout(time_limit, self.read_framed(buf)).await {
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
        match timeout(time_limit, self.write_framed(buf)).await {
            Ok(res) => res,
            Err(_) => Err(anyhow!("Writing to stream to {} timed out", self.addr())),
        }
    }

    async fn read_framed(&mut self, buf: &mut [u8]) -> Result<()> {
        if self.framing.is_none() {
            return self.read_raw(buf).await;
        }

        let mut filled = 0;
        while filled < buf.len() {
            let framing = self.framing.as_mut().unwrap();
            if framing.recv_pos == framing.recv_end {
                self.recv_frame().await?;
                continue;
            }

            let n = (framing.recv_end - framing.recv_pos).min(buf.len() - filled);
            buf[filled..filled + n]
                .copy_from_slice(&framing.recv_buf[framing.recv_pos..framing.recv_pos + n]);
            framing.recv_pos += n;
            filled += n;
        }

        Ok(())
    }

    async fn recv_frame(&mut self) -> Result<()> {
        let mut framing = self.framing.take().unwrap();
        let res = self.recv_frame_into(&mut framing).await;
        self.framing = Some(framing);
        res
    }

    async fn recv_frame_into(&mut self, framing: &mut Framing) -> Result<()> {
        framing.recv_pos = 0;
        framing.recv_end = 0;

        if framing.recv_buf.len() < FRAME_HDR_LEN {
            framing.recv_buf.resize(FRAME_HDR_LEN, 0);
        }
        self.read_raw(&mut framing.recv_buf[..FRAME_HDR_LEN])
            .await?;

        let len = peek_frame(&framing.recv_buf[..FRAME_HDR_LEN])?.len;
        if framing.recv_buf.len() < len {
            framing.recv_buf.resize(len, 0);
        }
        self.read_raw(&mut framing.recv_buf[FRAME_HDR_LEN..len])
            .await?;

        let (plain_len, _) = framing.transport.open(&mut framing.recv_buf[..len])?;
        framing.recv_pos = FRAME_HDR_LEN;
        framing.recv_end = FRAME_HDR_LEN + plain_len;

        Ok(())
    }

    async fn write_framed(&mut self, buf: &[u8]) -> Result<()> {
        let Some(mut framing) = self.framing.take() else {
            return self.write_raw(buf).await;
        };

        let res = async {
            let chunks = buf.len().div_ceil(MAX_PLAINTEXT_LEN).max(1);
            for (i, chunk) in buf.chunks(MAX_PLAINTEXT_LEN).enumerate() {
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

        let msg: Vec<u8> = (0..MAX_PLAINTEXT_LEN + 1000)
            .map(|i| (i * 7 + 3) as u8)
            .collect();
        let mut recv = vec![0; msg.len()];

        let (w, r) = tokio::join!(
            client.write_all(&msg, TL),
            server.read_exact(&mut recv[..10], TL)
        );
        w.unwrap();
        r.unwrap();
        server.readable().await.unwrap();
        server.read_exact(&mut recv[10..], TL).await.unwrap();
        assert_eq!(recv, msg);

        server.write_all(b"response", TL).await.unwrap();
        let mut resp = [0; 8];
        client.read_exact(&mut resp, TL).await.unwrap();
        assert_eq!(&resp, b"response");
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
