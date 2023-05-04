use super::msg_buffer::MsgBuffer;
use super::msg_dispatch::{DispatchRequest, SocketRequestChannel, StreamRequestChannel};
use super::stream::Stream;
use super::AddrResolver;
use crate::conn::PeerID;
use crate::msg::{self, Msg};
use crate::shutdown::Shutdown;
use crate::AuthenticationSecret;
use anyhow::{anyhow, bail, Result};
use std::collections::{HashMap, VecDeque};
use std::fmt::Debug;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;

#[derive(Debug)]
pub(crate) struct StreamHandle {
    pub(crate) _tx: UnboundedSender<()>,
}

#[derive(Debug, Default)]
pub struct ConnPoolConfig<A: AddrResolver> {
    pub stream_auth_secret: Option<AuthenticationSecret>,
    pub udp_sockets: Vec<Arc<UdpSocket>>,
    pub tcp_listeners: Vec<TcpListener>,
    pub addr_resolver: A,
}

#[derive(Clone, Debug)]
pub struct ConnPool<A: AddrResolver> {
    store_tx: UnboundedSender<Cmd>,
    addr_resolver: A,
    stream_auth_hash: Option<AuthenticationSecret>,
}

impl<A: AddrResolver> ConnPool<A> {
    async fn pop_stream(&self, peer_id: PeerID) -> Result<Option<Stream>> {
        let (tx, rx) = oneshot::channel();
        self.store_tx.send(Cmd::PopOutgoing(peer_id, tx))?;

        Ok(rx.await?)
    }

    async fn push_stream(&self, peer_id: PeerID, stream: Stream) -> Result<()> {
        self.store_tx.send(Cmd::PushOutgoing(peer_id, stream))?;
        Ok(())
    }

    async fn get_socket(&self) -> Result<Arc<UdpSocket>> {
        let (tx, rx) = oneshot::channel();
        self.store_tx.send(Cmd::GetSocket(tx))?;
        match rx.await? {
            Some(s) => Ok(s),
            None => Err(anyhow!("No usable socket found")),
        }
    }

    async fn pop_or_create_buffer(store_tx: &UnboundedSender<Cmd>) -> Result<MsgBuffer> {
        let (tx, rx) = oneshot::channel();
        store_tx.send(Cmd::PopBuffer(tx))?;

        let buf = rx.await?.unwrap_or_default();
        Ok(buf)
    }

    async fn push_buffer(store_tx: &UnboundedSender<Cmd>, buf: MsgBuffer) -> Result<()> {
        store_tx.send(Cmd::PushBuffer(buf))?;
        Ok(())
    }

    pub async fn request<M: msg::Msg, R: msg::Msg>(&self, peer_id: PeerID, msg: &M) -> Result<R> {
        let mut buf = Self::pop_or_create_buffer(&self.store_tx).await?;

        buf.serialize_msg(msg)?;
        self.do_request(peer_id, &mut buf, true).await?;
        let resp = buf.deserialize_msg()?;

        Self::push_buffer(&self.store_tx, buf).await?;

        Ok(resp)
    }

    pub async fn send<M: msg::Msg>(&self, peer_id: PeerID, msg: &M) -> Result<()> {
        let mut buf = Self::pop_or_create_buffer(&self.store_tx).await?;

        buf.serialize_msg(msg)?;
        self.do_request(peer_id, &mut buf, false).await?;

        Self::push_buffer(&self.store_tx, buf).await?;

        Ok(())
    }

    async fn do_request(
        &self,
        peer_id: PeerID,
        buf: &mut MsgBuffer,
        expect_response: bool,
    ) -> Result<()> {
        // pop tcp streams as long as there are some
        while let Some(mut stream) = self.pop_stream(peer_id).await? {
            match {
                buf.write_to_stream(&mut stream).await?;

                if expect_response {
                    buf.read_from_stream(&mut stream).await?;
                }
                Ok(()) as Result<()>
            } {
                Ok(_) => {
                    // request succeeded, re-push stream to connection pool
                    self.push_stream(peer_id, stream).await?;
                    return Ok(());
                }
                Err(err) => {
                    log::trace!("Stream send to {peer_id:?} failed:\n{err:?}");
                    continue;
                }
            }
        }

        // if there is none available or all available failed, try open a new one once on each route
        log::info!("Opening new stream to {:?}", peer_id);
        let addrs = self.addr_resolver.lookup(peer_id).await?;

        for a in addrs {
            match TcpStream::connect(a).await {
                Ok(stream) => {
                    let mut stream = Stream::new_tcp(stream, peer_id);

                    // Authenticate to the peer if required
                    if let Some(auth_secret) = self.stream_auth_hash {
                        let mut auth_buf = Self::pop_or_create_buffer(&self.store_tx).await?;
                        auth_buf.serialize_msg(&msg::AuthenticateChannel { auth_secret })?;
                        auth_buf.write_to_stream(&mut stream).await?;
                        Self::push_buffer(&self.store_tx, auth_buf).await?;
                    }

                    match {
                        buf.write_to_stream(&mut stream).await?;

                        if expect_response {
                            buf.read_from_stream(&mut stream).await?;
                        }
                        Ok(()) as Result<()>
                    } {
                        Ok(_) => {
                            // request succeeded, re-push stream to connection pool
                            self.push_stream(peer_id, stream).await?;
                            return Ok(());
                        }
                        Err(err) => {
                            log::trace!("Stream send to {peer_id:?} failed:\n{err:?}");
                            continue;
                        }
                    }
                }
                Err(_) => continue,
            }
        }

        bail!("Connecting to {peer_id:?} failed on all available routes")
    }

    pub async fn broadcast<M: Msg>(
        &self,
        peers: impl IntoIterator<Item = PeerID>,
        msg: &M,
    ) -> Result<()> {
        let mut buf = Self::pop_or_create_buffer(&self.store_tx).await?;
        buf.serialize_msg(msg)?;

        let sock = self.get_socket().await?;

        for k in peers {
            let addr = self.addr_resolver.lookup(k).await?;

            for a in addr {
                buf.send_to_socket(&sock, a).await?;
            }
        }

        Self::push_buffer(&self.store_tx, buf).await?;

        Ok(())
    }
}

#[derive(Debug)]
pub struct ConnPoolActor<A: AddrResolver> {
    store_tx: UnboundedSender<Cmd>,
    store_rx: UnboundedReceiver<Cmd>,
    udp_sockets: Vec<Arc<UdpSocket>>,
    tcp_listeners: Vec<TcpListener>,
    addr_resolver: A,
    stream_auth_hash: Option<AuthenticationSecret>,
}

impl<A: AddrResolver> ConnPoolActor<A> {
    pub fn new(config: ConnPoolConfig<A>) -> (Self, ConnPool<A>) {
        let (store_tx, store_rx) = unbounded_channel();

        (
            Self {
                store_tx: store_tx.clone(),
                store_rx,
                udp_sockets: config.udp_sockets,
                tcp_listeners: config.tcp_listeners,
                addr_resolver: config.addr_resolver.clone(),
                stream_auth_hash: config.stream_auth_secret,
            },
            ConnPool {
                store_tx,
                addr_resolver: config.addr_resolver,
                stream_auth_hash: config.stream_auth_secret,
            },
        )
    }

    pub fn start_tasks(self, msg_handler: impl DispatchRequest, shutdown: Shutdown) {
        for a in self.tcp_listeners {
            tokio::spawn(Self::tcp_listen_task(
                a,
                self.addr_resolver.clone(),
                msg_handler.clone(),
                self.store_tx.clone(),
                self.stream_auth_hash.is_some(),
                shutdown.clone(),
            ));
        }

        for s in &self.udp_sockets {
            tokio::spawn(Self::serve_udp_socket_task(
                s.clone(),
                msg_handler.clone(),
                self.store_tx.clone(),
                shutdown.clone(),
            ));
        }

        tokio::spawn(store_task(self.udp_sockets, self.store_rx, shutdown));
    }

    async fn tcp_listen_task(
        listener: TcpListener,
        addr_resolver: impl AddrResolver,
        msg_handler: impl DispatchRequest,
        store_tx: UnboundedSender<Cmd>,
        stream_authentication_required: bool,
        mut shutdown: Shutdown,
    ) {
        loop {
            tokio::select! {
                res = listener.accept() => {
                    let (stream, addr) = match res {
                        Ok(res) => res,
                        Err(err) => {
                            log::error!("Accepting TCP connection failed: {err:#}");
                            continue;
                        }
                    };

                    let peer_id = addr_resolver.reverse_lookup(addr).await;

                    let (tx, rx) = unbounded_channel();

                    if store_tx.send(Cmd::PushIncoming(StreamHandle { _tx: tx })).is_err() {
                        break;
                    }

                    tokio::spawn(Self::serve_stream_task(
                        Stream::new_tcp(stream, peer_id),
                        rx,
                        msg_handler.clone(),
                        stream_authentication_required,
                    ));
                }

                _ = shutdown.wait() =>{ break; }
            }
        }

        log::debug!("TCP listener task has been shut down: {listener:?}")
    }

    async fn serve_stream_task(
        mut stream: Stream,
        mut conn_rx: UnboundedReceiver<()>,
        mut msg_handler: impl DispatchRequest,
        stream_authentication_required: bool,
    ) {
        log::debug!("Accepted incoming stream from {:?}", stream.peer_id);

        let mut buf = MsgBuffer::default();

        loop {
            tokio::select! {
                _ = stream.readable() => {
                    if let Err(err) = Self::on_incoming_stream(&mut stream, &mut buf, &mut msg_handler, stream_authentication_required).await {
                        log::debug!("Closed stream from {:?}: {err:#}", stream.peer_id);
                        break;
                    }
                }
                _ = conn_rx.recv() => {
                    log::debug!("Closed stream from {:?}: Connection handle dropped", stream.peer_id);

                    break;
                }
            }
        }

        log::debug!(
            "Stream handler task has been shut down: {:?}",
            stream.peer_id
        )
    }

    async fn on_incoming_stream(
        stream: &mut Stream,
        buf: &mut MsgBuffer,
        msg_handler: &mut impl DispatchRequest,
        stream_authentication_required: bool,
    ) -> Result<()> {
        buf.read_from_stream(stream).await?;

        // check authentication
        if stream_authentication_required
            && !stream.authenticated
            && buf.msg_id() != msg::AuthenticateChannel::ID
        {
            bail!("Unauthenticated stream from {:?}", stream.peer_id);
        }

        let req = StreamRequestChannel {
            stream,
            msg_buf: buf,
        };

        msg_handler.dispatch_request(req).await?;

        Ok(())
    }

    async fn serve_udp_socket_task(
        sock: Arc<UdpSocket>,
        msg_handler: impl DispatchRequest,
        store_tx: UnboundedSender<Cmd>,
        mut shutdown: Shutdown,
    ) {
        loop {
            tokio::select! {
                res = Self::on_incoming_datagram(sock.clone(), msg_handler.clone(), store_tx.clone()) => {
                    if let Err(err) = res {
                        log::error!("Error in UDP socket {sock:?}: {err:#}");
                        break;
                    }
                }

                _ = shutdown.wait() => { break; }
            }
        }

        log::debug!("Socket handler task has been shut down: {sock:?}")
    }

    async fn on_incoming_datagram(
        sock: Arc<UdpSocket>,
        mut msg_handler: impl DispatchRequest,
        store_tx: UnboundedSender<Cmd>,
    ) -> Result<()> {
        let mut buf = ConnPool::<A>::pop_or_create_buffer(&store_tx).await?;
        let peer_addr = buf.recv_from_socket(&sock).await?;

        tokio::spawn(async move {
            let req = SocketRequestChannel {
                sock,
                peer_addr,
                msg_buf: &mut buf,
            };

            let _ = msg_handler.dispatch_request(req).await;
            let _ = ConnPool::<A>::push_buffer(&store_tx, buf).await;
        });

        Ok(())
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub(super) enum Cmd {
    PushOutgoing(PeerID, Stream),
    PopOutgoing(PeerID, oneshot::Sender<Option<Stream>>),
    PushIncoming(StreamHandle),
    GetSocket(oneshot::Sender<Option<Arc<UdpSocket>>>),
    PushBuffer(MsgBuffer),
    PopBuffer(oneshot::Sender<Option<MsgBuffer>>),
}

async fn store_task(
    sockets: Vec<Arc<UdpSocket>>,
    mut rx: UnboundedReceiver<Cmd>,
    mut shutdown: Shutdown,
) {
    let mut outbound_conns = HashMap::<PeerID, Vec<Stream>, _>::new();
    let mut inbound_handles = Vec::<StreamHandle>::new();
    let mut msg_buffers = VecDeque::<MsgBuffer>::new();

    loop {
        tokio::select! {
            cmd = rx.recv() => {
                match cmd {
                    Some(cmd) => {
                        match cmd {
                            Cmd::PushOutgoing(generic_addr, stream) => {
                                match outbound_conns.get_mut(&generic_addr) {
                                    Some(c) => c.push(stream),
                                    None => {
                                        outbound_conns.insert(generic_addr, vec![stream]);
                                    }
                                }
                            }
                            Cmd::PopOutgoing(generic_addr, tx) => {
                                let conn = outbound_conns.get_mut(&generic_addr).and_then(|e| e.pop());
                                let _ = tx.send(conn);
                            }
                            Cmd::PushIncoming(handle) => {
                                inbound_handles.push(handle);
                            }
                            Cmd::GetSocket(tx) => {
                                let _ = tx.send(sockets.first().cloned());
                            }
                            Cmd::PushBuffer(buf) => {
                                msg_buffers.push_back(buf);
                            }
                            Cmd::PopBuffer(tx) => {
                                let buf = msg_buffers.pop_front();
                                let _ = tx.send(buf);
                            }
                        }
                    }

                    None => { break; }
                }

            }

            _ = shutdown.wait() => { break; }
        }
    }

    log::debug!("Connection pool store task has been shut down");
}
