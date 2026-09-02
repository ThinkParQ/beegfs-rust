//! Handle incoming TCP and UDP connections and BeeMsgs.

use super::msg_dispatch::{DispatchRequest, SocketRequest, StreamRequest};
use super::protocol::Protocol;
use super::stream::Stream;
use super::{handshake, *};
use crate::bee_msg::misc::AuthenticateChannel;
use crate::bee_msg::{Header, Msg};
use crate::run_state::RunStateHandle;
use anyhow::{Context, Result, bail};
use std::io::{self, ErrorKind};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, UdpSocket};

/// Spawns a new task that listens for incoming TCP connections. The task accepts all connection
/// requests and spawns a new receiver task for each of them, handling receiving BeeMsges and
/// forwarding them to the provided dispatcher. This is probably what you want to call if you want
/// to receive and process BeeMsgs.
///
/// The `dispatch` argument expects an implementation of [`DispatchRequest`] and is called whenever
/// a BeeMsg is received.
///
/// `cfg` selects the wire protocol and, for the legacy one, whether a [`Stream`] must have the
/// authenticated flag set before any message other than [`AuthenticateChannel`] is accepted. It is
/// up to the handler to set the flag while handling that message.
///
/// The [`Shutdown`] handle is used to shutdown all running tasks gracefully (e.g. finishing running
/// operations)
///
/// There is no connection limit on incoming connections.
///
/// # Return behavior
/// Returns the actually bound address immediately after the task has been started.
pub async fn listen_tcp(
    listen_addr: SocketAddr,
    dispatch: impl DispatchRequest,
    protocol: Protocol,
    mut run_state: RunStateHandle,
    lookup: impl Lookup,
) -> Result<SocketAddr> {
    let listener = TcpListener::bind(listen_addr).await?;
    let bound_addr = listener.local_addr()?;
    log::info!("Listening for BeeGFS connections on {bound_addr}");

    tokio::spawn(async move {
        // Listen-loop
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
                    tokio::spawn(stream_loop(
                        Stream::from_tcpstream(stream, &protocol),
                        dispatch.clone(),
                        protocol.clone(),
                        run_state.clone(),
                        lookup.clone(),
                    ));
                }

                _ = run_state.wait_for_shutdown() =>{ break; }
            }
        }

        log::debug!("TCP listener task has been shut down: {listener:?}")
    });

    Ok(bound_addr)
}

/// Contains the stream reading loop
async fn stream_loop(
    mut stream: Stream,
    dispatch: impl DispatchRequest,
    protocol: Protocol,
    mut run_state: RunStateHandle,
    lookup: impl Lookup,
) {
    log::debug!("Accepted incoming stream from {:?}", stream.addr());

    if let Protocol::Protected(ref p) = protocol {
        match handshake::respond(&mut stream, p, &lookup).await {
            Ok(peer) => log::debug!(
                "Stream from {:?} authenticated as identity {}",
                stream.addr(),
                peer.identity.name
            ),
            Err(err) => {
                log::warn!(
                    "Key exchange with {:?} failed, closing the stream: {err:#}",
                    stream.addr()
                );
                return;
            }
        }
    }

    // Use one owned buffer for reading into and writing from.
    let mut buf = vec![0; TCP_BUF_LEN];

    loop {
        // Wait for available data or shutdown signal
        tokio::select! {
            res = stream.readable() => {
                if let Err(err) = res {
                    log::debug!("Closed stream from {:?}: {err:#}", stream.addr());
                    return;
                }
            }
            _ = run_state.wait_for_shutdown() => {
                return;
            }
        }

        if let Err(err) = read_stream(&mut stream, &mut buf, &dispatch, &protocol).await {
            // If the error comes from the connection being closed, we only log a debug message
            if let Some(inner) = err.downcast_ref::<io::Error>()
                && let ErrorKind::UnexpectedEof = inner.kind()
            {
                log::debug!("Closed stream from {:?}: {err:#}", stream.addr());
                return;
            }

            log::error!(
                "Error while handling stream from {:?}: {err:#}",
                stream.addr()
            );
            return;
        }
    }
}

/// Reads in a message from the given stream into the given buffer and forwards it to the
/// dispatcher. Checks the authentication flag on the [`Stream`] if the legacy protocol requires it.
///
/// The dispatcher is responsible for deserializing the message, dispatching it to the correct
/// handler and sending back a response using the [`StreamRequest`] handle.
async fn read_stream(
    stream: &mut Stream,
    buf: &mut [u8],
    dispatch: &impl DispatchRequest,
    protocol: &Protocol,
) -> Result<()> {
    let header = stream.read_msg(buf, GENERIC_STREAM_TIME_LIMIT).await?;

    // Only the legacy protocol authenticates per message - the new one gates the whole stream
    // during connection setup.
    if let Protocol::Legacy(Some(_)) = protocol
        && !stream.authenticated
        && header.msg_id() != AuthenticateChannel::ID
    {
        bail!(
            "Stream is not authenticated and received message with id {}",
            header.msg_id()
        );
    }

    // Forward to the dispatcher. The dispatcher is responsible for deserializing, dispatching to
    // msg handlers and sending a response using the [`StreamRequest`] handle.
    dispatch
        .dispatch_request(StreamRequest {
            stream,
            buf,
            header: &header,
        })
        .await
        .context("Stream msg dispatch failed")?;

    Ok(())
}

/// Spawns a new task that receives datagrams from a UDP socket and forwards them to the
/// dispatcher. This is probably what you want to call if you want to receive and process BeeMsgs
/// via UDP.
///
/// The `dispatch` argument expects an implementation of [`DispatchRequest`] and is called whenever
/// a BeeMsg is received.
///
/// The [`Shutdown`] handle is used to shutdown all running tasks gracefully (e.g. finishing running
/// operations)
///
/// # Return behavior
/// Returns immediately after the task has been started.
pub fn recv_udp(
    sock: Arc<UdpSocket>,
    dispatch: impl DispatchRequest,
    mut run_state: RunStateHandle,
) -> Result<()> {
    log::info!("Receiving BeeGFS datagrams on {}", sock.local_addr()?);

    tokio::spawn(async move {
        // Receive loop
        loop {
            tokio::select! {
                // Do the actual work
                res = recv_datagram(sock.clone(), dispatch.clone()) => {
                    if let Err(err) = res {
                        log::error!("Error on receiving datagram using UDP socket {:?}: {err:#}", sock.local_addr());
                    }
                }

                _ = run_state.wait_for_shutdown() => { break; }
            }
        }

        log::debug!("UDP receiver task has been shut down: {sock:?}")
    });

    Ok(())
}

/// Receives a datagram from the given socket into and forwards it to the dispatcher.
///
/// The dispatcher is responsible for deserializing the message, dispatching it to the correct
/// handler and sending back a message using the [`SocketRequest`] handle.
async fn recv_datagram(sock: Arc<UdpSocket>, msg_handler: impl DispatchRequest) -> Result<()> {
    // We use a new buffer for each incoming datagram. This is not ideal, but since each incoming
    // message spawns a new task (below) and we don't know how long the processing takes, we cannot
    // reuse Buffers like the TCP reader does.
    // A separate buffer pool could potentially be used to avoid allocating new buffers every time.
    let mut buf = vec![0; UDP_BUF_LEN];

    let (_, peer_addr) = sock.recv_from(&mut buf).await?;

    // Request shall be handled in a separate task, so the next datagram can be processed
    // immediately
    tokio::spawn(async move {
        if let Err(err) = async {
            let header = Header::deserialize_legacy(&buf)?;

            let req = SocketRequest {
                sock,
                peer_addr,
                buf: &mut buf,
                header: &header,
            };

            // Forward to the dispatcher
            msg_handler.dispatch_request(req).await?;

            Ok::<(), anyhow::Error>(())
        }
        .await
        {
            log::error!("Error while handling datagram from {peer_addr:?}: {err:#}");
        }
    });

    Ok(())
}
