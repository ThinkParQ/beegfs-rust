//! Handle incoming TCP and UDP connections and BeeMsgs.

use super::msg_buf::MsgBuf;
use super::msg_dispatch::{DispatchRequest, SocketRequest, StreamRequest};
use super::stream::Stream;
use crate::bee_msg::Msg;
use crate::bee_msg::misc::AuthenticateChannel;
use crate::run_state::RunStateHandle;
use anyhow::{Result, bail};
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
    mut run_state: RunStateHandle,
) -> Result<()> {
    let listener = TcpListener::bind(listen_addr).await?;
    log::info!("Listening for BeeGFS connections on {listen_addr}");

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
                        stream.into(),
                        dispatch.clone(),
                        stream_authentication_required,
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
    mut run_state: RunStateHandle,
) {
    log::debug!("Accepted incoming stream from {:?}", stream.addr());

    // Use one owned buffer for reading into and writing from.
    let mut buf = MsgBuf::default();

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
            if let Some(inner) = err.downcast_ref::<io::Error>() {
                if let ErrorKind::UnexpectedEof = inner.kind() {
                    log::debug!("Closed stream from {:?}: {err:#}", stream.addr());
                    return;
                }
            }

            log::error!("Closed stream from {:?}: {err:#}", stream.addr());
            return;
        }
    }
}

/// Reads in data from the given stream into the given buffer and forwards it to the dispatcher.
/// Checks the authentication flag on the [`Stream`] if `stream_authentication_required` is set.
///
/// The dispatcher is responsible for deserializing the message, dispatching it to the correct
/// handler and sending back a response using the [`StreamRequest`] handle.
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
    // msg handlers and sending a response using the [`StreamRequest`] handle.
    dispatch
        .dispatch_request(StreamRequest { stream, buf })
        .await?;

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
                        log::error!("Error in UDP socket {sock:?}: {err:#}");
                        break;
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

        // Forward to the dispatcher
        let _ = msg_handler.dispatch_request(req).await;
    });

    Ok(())
}
