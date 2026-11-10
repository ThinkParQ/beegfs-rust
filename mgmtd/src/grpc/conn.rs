use crate::app::{App, RuntimeApp};
use anyhow::{Context, Result, bail};
use rusqlite::OptionalExtension;
use shared::conn::PeerIdentity;
use shared::conn::beecrypt::rustls::ServerConfig;
use shared::conn::beecrypt::rustls::server::Acceptor;
use shared::conn::beecrypt::{KeyPairId, Mode, grpc_mode_alpn};
use shared::run_state::RunStateHandle;
use sqlite::TransactionExt;
use sqlite_check::sql;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_rustls::LazyConfigAcceptor;
use tokio_rustls::server::TlsStream;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::server::Connected;

const HANDSHAKE_TIME_LIMIT: Duration = Duration::from_secs(5);
const TLS_HANDSHAKE_RECORD: u8 = 0x16;
const GRPC_MODE_ALPN_PREFIX: &[u8] = b"beegfs-grpc/1/";

#[derive(Clone, Debug)]
pub(crate) struct ConnInfo {
    pub addr: SocketAddr,
    pub identity: Arc<Mutex<PeerIdentity>>,
    pub admin: Arc<AtomicBool>,
}

pub(crate) struct Conn {
    inner: TlsStream<TcpStream>,
    info: ConnInfo,
}

impl Connected for Conn {
    type ConnectInfo = ConnInfo;

    fn connect_info(&self) -> Self::ConnectInfo {
        self.info.clone()
    }
}

impl AsyncRead for Conn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for Conn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[derive(Clone)]
pub(crate) struct TlsSetup {
    config: Arc<ServerConfig>,
    mismatch: Arc<ServerConfig>,
    mode: Mode,
}

impl TlsSetup {
    pub(crate) fn new(config: Arc<ServerConfig>, mode: Mode) -> Self {
        let mut mismatch = (*config).clone();
        mismatch.alpn_protocols = vec![grpc_mode_alpn(mode).to_vec()];

        Self {
            config,
            mismatch: Arc::new(mismatch),
            mode,
        }
    }
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Plain => "plain",
        Mode::Encrypted => "encrypted",
    }
}

pub(crate) fn listen(
    app: RuntimeApp,
    listener: TcpListener,
    tls: TlsSetup,
    mut run_state: RunStateHandle,
) -> ReceiverStream<io::Result<Conn>> {
    let (tx, rx) = mpsc::channel(16);

    tokio::spawn(async move {
        loop {
            tokio::select! {
                res = listener.accept() => {
                    let (stream, addr) = match res {
                        Ok(res) => res,
                        Err(err) => {
                            log::error!("Accepting gRPC connection failed: {err:#}");
                            continue;
                        }
                    };

                    let (app, tls, tx) = (app.clone(), tls.clone(), tx.clone());
                    tokio::spawn(async move {
                        match accept(&app, &tls, stream, addr).await {
                            Ok(conn) => {
                                let _ = tx.send(Ok(conn)).await;
                            }
                            Err(err) => log::error!("Establishing gRPC connection from {addr} failed: {err:#}"),
                        }
                    });
                }

                _ = run_state.wait_for_shutdown() => { break; }
            }
        }
    });

    ReceiverStream::new(rx)
}

async fn handshake(tls: &TlsSetup, stream: TcpStream) -> Result<TlsStream<TcpStream>> {
    let mut first = [0u8; 1];
    if stream.peek(&mut first).await? == 0 {
        bail!("Client closed the connection before the TLS handshake");
    }
    if first[0] != TLS_HANDSHAKE_RECORD {
        bail!(
            "Client did not start a TLS handshake. It probably uses the legacy connection \
            protocol with TLS disabled, management uses {}",
            mode_name(tls.mode)
        );
    }

    let start = LazyConfigAcceptor::new(Acceptor::default(), stream)
        .await
        .context("Reading the TLS ClientHello failed")?;

    let expected = grpc_mode_alpn(tls.mode);
    let offered: Vec<Vec<u8>> = start
        .client_hello()
        .alpn()
        .map(|protocols| protocols.map(<[u8]>::to_vec).collect())
        .unwrap_or_default();

    if offered.iter().any(|p| p.as_slice() == expected) {
        return start
            .into_stream(tls.config.clone())
            .await
            .context("TLS handshake failed");
    }

    let _ = start.into_stream(tls.mismatch.clone()).await;

    let client_mode = offered.iter().find_map(|p| {
        p.strip_prefix(GRPC_MODE_ALPN_PREFIX)
            .map(|m| String::from_utf8_lossy(m).into_owned())
    });

    match client_mode {
        Some(m) => bail!(
            "Client uses the {m} connection protocol, management uses {}",
            mode_name(tls.mode)
        ),
        None => bail!(
            "Client uses the legacy connection protocol, management uses {}",
            mode_name(tls.mode)
        ),
    }
}

async fn accept(
    app: &RuntimeApp,
    tls: &TlsSetup,
    stream: TcpStream,
    addr: SocketAddr,
) -> Result<Conn> {
    let inner = timeout(HANDSHAKE_TIME_LIMIT, handshake(tls, stream))
        .await
        .context("TLS handshake timed out")??;

    let cert = inner
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|certs| certs.first())
        .context("Peer presented no certificate")?;
    let key_id = KeyPairId::from_cert(cert)?;

    let registered: Option<(i64, bool)> = app
        .read_tx(move |tx| {
            Ok(tx
                .query_row_cached(
                    sql!(
                        "SELECT k.identity_id, i.admin FROM keys AS k
                        INNER JOIN identities AS i USING (identity_id)
                        WHERE k.key = ?1 AND i.revoked = 0"
                    ),
                    [key_id.to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?)
        })
        .await
        .context("Identity lookup failed")?;

    let (identity, admin) = match registered {
        Some((identity_id, admin)) => (
            PeerIdentity::Registered {
                key_id,
                identity_id,
            },
            admin,
        ),
        None => (PeerIdentity::Unregistered(key_id), false),
    };

    log::debug!("Established gRPC connection from {addr} with identity {identity:?}");

    Ok(Conn {
        inner,
        info: ConnInfo {
            addr,
            identity: Arc::new(Mutex::new(identity)),
            admin: Arc::new(AtomicBool::new(admin)),
        },
    })
}
