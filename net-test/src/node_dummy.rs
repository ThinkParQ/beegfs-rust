use crate::msg_store::MsgStore;
use shared::conn::{ConnPool, ConnPoolActor, ConnPoolConfig, SocketAddrResolver};
use shared::shutdown::ShutdownControl;
use shared::types::{AckID, Port};
use shared::*;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, UdpSocket};

const MGMTD_ADDR: &str = "127.0.0.1";
const MGMTD_PORT: u16 = 8008;

#[derive(Debug)]
pub struct NodeDummy {
    conn: ConnPool<SocketAddrResolver>,
    pub msg_store: MsgStore,
    _shutdown_control: ShutdownControl,
}

impl NodeDummy {
    pub async fn run(port: Port) -> Self {
        let msg_store = MsgStore::new();

        let (conn_pool_actor, conn) = ConnPoolActor::new(ConnPoolConfig {
            stream_auth_secret: None,
            udp_sockets: vec![Arc::new(
                UdpSocket::bind(SocketAddr::new("0.0.0.0".parse().unwrap(), port.into()))
                    .await
                    .unwrap(),
            )],
            tcp_listeners: vec![TcpListener::bind(SocketAddr::new(
                "0.0.0.0".parse().unwrap(),
                port.into(),
            ))
            .await
            .unwrap()],
            addr_resolver: SocketAddrResolver {},
        });

        let (shutdown, _shutdown_control) = shutdown::new();

        conn_pool_actor.start_tasks(msg_store.clone(), shutdown);

        Self {
            conn,
            msg_store,
            _shutdown_control,
        }
    }

    pub async fn request<M: msg::Msg, R: msg::Msg>(&self, msg: M) -> R {
        self.conn
            .request(
                PeerID::Addr(SocketAddr::new(MGMTD_ADDR.parse().unwrap(), MGMTD_PORT)),
                msg,
            )
            .await
            .unwrap()
    }

    pub async fn send<M: msg::Msg>(&self, msg: M) {
        self.conn
            .send(
                PeerID::Addr(SocketAddr::new(MGMTD_ADDR.parse().unwrap(), MGMTD_PORT)),
                msg,
            )
            .await
            .unwrap();
    }

    pub async fn send_datagram<M: msg::Msg>(&self, msg: M, ack_id: Option<AckID>) {
        self.conn
            .broadcast(
                &mut [PeerID::Addr(SocketAddr::new(
                    MGMTD_ADDR.parse().unwrap(),
                    MGMTD_PORT,
                ))]
                .into_iter(),
                msg,
            )
            .await
            .unwrap();

        if let Some(ack_id) = ack_id {
            self.msg_store.wait_for_ack(ack_id.clone()).await.unwrap();
        }
    }
}
