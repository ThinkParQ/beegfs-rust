use crate::app::RuntimeApp;
use anyhow::{Context, Result};
use shared::conn::PeerIdentity;
use shared::conn::beecrypt::KeyPairId;
use shared::conn::msg_dispatch::DispatchRequest;
use shared::run_state::RunStateHandle;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::server::Connected;

const HANDSHAKE_TIME_LIMIT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
pub(crate) struct ConnInfo {
    pub addr: SocketAddr,
    pub identity: PeerIdentity,
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

pub(crate) fn listen(
    app: RuntimeApp,
    listener: TcpListener,
    acceptor: TlsAcceptor,
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

                    let (app, acceptor, tx) = (app.clone(), acceptor.clone(), tx.clone());
                    tokio::spawn(async move {
                        match accept(&app, &acceptor, stream, addr).await {
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

async fn accept(
    app: &RuntimeApp,
    acceptor: &TlsAcceptor,
    stream: TcpStream,
    addr: SocketAddr,
) -> Result<Conn> {
    let inner = timeout(HANDSHAKE_TIME_LIMIT, acceptor.accept(stream))
        .await
        .context("TLS handshake timed out")?
        .context("TLS handshake failed")?;

    let cert = inner
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|certs| certs.first())
        .context("Peer presented no certificate")?;
    let key_id = KeyPairId::from_cert(cert)?;

    let identity = match app
        .lookup_identity(key_id)
        .await
        .context("Identity lookup failed")?
    {
        Some(identity_id) => PeerIdentity::Registered {
            key_id,
            identity_id,
        },
        None => PeerIdentity::Unregistered(key_id),
    };

    log::debug!("Established gRPC connection from {addr} with identity {identity:?}");

    Ok(Conn {
        inner,
        info: ConnInfo { addr, identity },
    })
}
