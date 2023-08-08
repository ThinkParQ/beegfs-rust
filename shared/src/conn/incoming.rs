use super::msg_buf::MsgBuf;
use super::msg_dispatch::{DispatchRequest, SocketRequest, StreamRequest};
use super::stream::Stream;
use crate::msg::{self, Msg};
use crate::shutdown::Shutdown;
use anyhow::{bail, Result};
use std::sync::Arc;
use tokio::net::{TcpListener, UdpSocket};

pub async fn listen_tcp(
    listener: TcpListener,
    dispatch: impl DispatchRequest,
    stream_authentication_required: bool,
    mut shutdown: Shutdown,
) {
    loop {
        tokio::select! {
            res = listener.accept() => {
                let (stream, _) = match res {
                    Ok(res) => res,
                    Err(err) => {
                        log::error!("Accepting TCP connection failed: {err:#}");
                        continue;
                    }
                };

                tokio::spawn(new_stream(
                    stream.into(),
                    dispatch.clone(),
                    stream_authentication_required,
                ));
            }

            _ = shutdown.wait() =>{ break; }
        }
    }

    log::debug!("TCP listener task has been shut down: {listener:?}")
}

async fn new_stream(
    mut stream: Stream,
    dispatch: impl DispatchRequest,
    stream_authentication_required: bool,
) {
    log::debug!("Accepted incoming stream from {:?}", stream.addr());

    let mut buf = MsgBuf::default();

    loop {
        if let Err(err) = stream.readable().await {
            log::debug!("Closed stream from {:?}: {err}", stream.addr());
            return;
        }

        if let Err(err) = read_stream(
            &mut stream,
            &mut buf,
            &dispatch,
            stream_authentication_required,
        )
        .await
        {
            log::debug!("Closed stream from {:?}: {err}", stream.addr());
            return;
        }
    }
}

async fn read_stream(
    stream: &mut Stream,
    buf: &mut MsgBuf,
    dispatch: &impl DispatchRequest,
    stream_authentication_required: bool,
) -> Result<()> {
    buf.read_from_stream(stream).await?;

    // check authentication
    if stream_authentication_required
        && !stream.authenticated
        && buf.msg_id() != msg::AuthenticateChannel::ID
    {
        bail!("Unauthenticated stream from {:?}", stream.addr());
    }

    let req = StreamRequest {
        stream,
        msg_buf: buf,
    };

    dispatch.dispatch_request(req).await?;

    Ok(())
}

pub async fn recv_udp(
    sock: Arc<UdpSocket>,
    dispatch: impl DispatchRequest,
    mut shutdown: Shutdown,
) {
    loop {
        tokio::select! {
            res = recv_datagram(sock.clone(), dispatch.clone()) => {
                if let Err(err) = res {
                    log::error!("Error in UDP socket {sock:?}: {err:#}");
                    break;
                }
            }

            _ = shutdown.wait() => { break; }
        }
    }

    log::debug!("UDP receiver task has been shut down: {sock:?}")
}

async fn recv_datagram(sock: Arc<UdpSocket>, msg_handler: impl DispatchRequest) -> Result<()> {
    let mut buf = MsgBuf::default();
    let peer_addr = buf.recv_from_socket(&sock).await?;

    tokio::spawn(async move {
        let req = SocketRequest {
            sock,
            peer_addr,
            msg_buf: &mut buf,
        };

        let _ = msg_handler.dispatch_request(req).await;
    });

    Ok(())
}
