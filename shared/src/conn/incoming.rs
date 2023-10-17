//! Handle incoming TCP and UDP connections and data

use super::msg_buf::MsgBuf;
use super::msg_dispatch::{DispatchRequest, SocketRequest, StreamRequest};
use super::stream::Stream;
use crate::msg::authenticate_channel::AuthenticateChannel;
use crate::msg::Msg;
use crate::shutdown::Shutdown;
use anyhow::{bail, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, UdpSocket};

/// Listens for new TCP connections
pub async fn listen_tcp(
    listen_addr: SocketAddr,
    dispatch: impl DispatchRequest,
    stream_authentication_required: bool,
    mut shutdown: Shutdown,
) -> Result<()> {
    let listener = TcpListener::bind(listen_addr).await?;
    log::info!("Listening for BeeGFS connections on {listen_addr}");

    tokio::spawn(async move {
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

                    // BeeGFS streams follow a "request-response" schema: A request is made using one
                    // stream and the following response comes back using the same stream. The stream
                    // is blocked during that and not used for anything else. Therefore, we just handle
                    // reading from each stream in a separate task that is also used for
                    // (de-)serializing, processing the request and sending the response.
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
    });

    Ok(())
}

/// Handles an incoming stream
async fn new_stream(
    mut stream: Stream,
    dispatch: impl DispatchRequest,
    stream_authentication_required: bool,
) {
    log::debug!("Accepted incoming stream from {:?}", stream.addr());

    let mut buf = MsgBuf::default();

    loop {
        // Wait for data being written to the "wire"
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

/// Reads in data from a stream and forwards it to the dispatcher
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
        && buf.msg_id() != AuthenticateChannel::ID
    {
        bail!(
            "Received message on unauthenticated stream from {:?}",
            stream.addr()
        );
    }

    // Forward to the dispatcher. The dispatcher is responsible for deserializing, dispatching to
    // msg handlers and sending a response using the [StreamRequest] handle.
    dispatch
        .dispatch_request(StreamRequest { stream, buf })
        .await?;

    Ok(())
}

/// Receives datagrams from a UDP socket
pub fn recv_udp(
    sock: Arc<UdpSocket>,
    dispatch: impl DispatchRequest,
    mut shutdown: Shutdown,
) -> Result<()> {
    log::info!("Receiving BeeGFS datagrams on {}", sock.local_addr()?);

    tokio::spawn(async move {
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
    });

    Ok(())
}

/// Receives a datagram and forwards it to the dispatcher
async fn recv_datagram(sock: Arc<UdpSocket>, msg_handler: impl DispatchRequest) -> Result<()> {
    let mut buf = MsgBuf::default();
    let peer_addr = buf.recv_from_socket(&sock).await?;

    // Request shall be handled in a separate task, so the next datagram can be processed
    // immediately
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
