//! Handle incoming TCP and UDP connections and BeeMsgs.

use super::msg_dispatch::{DispatchRequest, SocketRequest, StreamRequest};
use super::stream::Stream;
use super::*;
use crate::bee_msg::misc::{AuthenticateChannel, RegisterIdentity};
use crate::bee_msg::{Msg, deserialize_legacy_header};
use crate::run_state::RunStateHandle;
use anyhow::{Context, Result, bail};
use beecrypt::Role;
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
/// `stream_authentication_required` control on whether a [`Stream`] must have set to authenticated
/// flag when receiving any other message than [`AuthenticateChannel`]. It is up to the handler to
/// set the flag while handling this message.
///
/// The [`Shutdown`] handle is used to shutdown all running tasks gracefully (e.g. finishing running
/// operations)
///
/// There is no connection limit on incoming connections.
///
/// # Return behavior
/// Returns immediately after the task has been started.
pub async fn listen_tcp(
    listen_addr: SocketAddr,
    dispatch: impl DispatchRequest,
    stream_authentication_required: bool,
    crypto: Option<ConnCrypto>,
    mut run_state: RunStateHandle,
) -> Result<()> {
    let listener = TcpListener::bind(listen_addr).await?;
    log::info!("Listening for BeeGFS connections on {listen_addr}");

    tokio::spawn(async move {
        // Listen-loop
        loop {
            tokio::select! {
                res = listener.accept() => {
                    let (stream, peer_addr) = match res {
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
                        Stream::from_tcp_stream(stream, peer_addr),
                        dispatch.clone(),
                        stream_authentication_required,
                        crypto.clone(),
                        run_state.clone(),
                    ));
                }

                _ = run_state.wait_for_shutdown() =>{ break; }
            }
        }

        log::debug!("TCP listener task has been shut down: {listener:?}")
    });

    Ok(())
}

/// Contains the stream reading loop
async fn stream_loop(
    mut stream: Stream,
    dispatch: impl DispatchRequest,
    stream_authentication_required: bool,
    crypto: Option<ConnCrypto>,
    mut run_state: RunStateHandle,
) {
    log::debug!("Accepted incoming stream from {:?}", stream.addr());

    if let Some(crypto) = crypto
        && let Err(err) = accept_identity(&mut stream, &dispatch, &crypto).await
    {
        log::error!(
            "Establishing stream from {:?} failed: {err:#}",
            stream.addr()
        );
        return;
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

        if let Err(err) = read_stream(
            &mut stream,
            &mut buf,
            &dispatch,
            stream_authentication_required,
        )
        .await
        {
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

async fn accept_identity(
    stream: &mut Stream,
    dispatch: &impl DispatchRequest,
    crypto: &ConnCrypto,
) -> Result<()> {
    let key_id = stream
        .handshake(Role::Responder, crypto, None, GENERIC_STREAM_TIME_LIMIT)
        .await
        .context("Handshake failed")?;

    stream.identity = match dispatch
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

    log::debug!(
        "Established stream from {:?} with identity {:?}",
        stream.addr(),
        stream.identity
    );

    Ok(())
}

/// Reads in data from the given stream into the given buffer and forwards it to the dispatcher.
/// Checks the authentication flag on the [`Stream`] if `stream_authentication_required` is set.
///
/// The dispatcher is responsible for deserializing the message, dispatching it to the correct
/// handler and sending back a response using the [`StreamRequest`] handle.
async fn read_stream(
    stream: &mut Stream,
    buf: &mut [u8],
    dispatch: &impl DispatchRequest,
    stream_authentication_required: bool,
) -> Result<()> {
    let header = stream.read_msg(buf, GENERIC_STREAM_TIME_LIMIT).await?;

    // check authentication
    if stream_authentication_required
        && !stream.authenticated
        && header.msg_id() != AuthenticateChannel::ID
    {
        bail!(
            "Stream is not authenticated and received message with id {}",
            header.msg_id()
        );
    }

    if let PeerIdentity::Unregistered(key_id) = stream.identity
        && header.msg_id() != RegisterIdentity::ID
    {
        bail!(
            "Stream peer with key pair ID {key_id} is not registered and sent message with id {}",
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
            let header = deserialize_legacy_header(&buf)?;

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
