mod common;
use common::*;
use shared::AuthenticationSecret;
use std::net::Ipv4Addr;

#[net_test]
async fn heartbeat() {
    let n = NodeDummy::run(Port::from(9000)).await;

    n.send_datagram(
        msg::Heartbeat {
            instance_version: 0,
            nic_list_version: 0,
            node_type: NodeType::Meta,
            node_alias: "TestNode".try_into().unwrap(),
            ack_id: "ACKID".into(),
            node_num_id: NodeID::from(123),
            root_num_id: 1,
            is_root_mirrored: false,
            port: Port::from(9000),
            port_tcp_unused: Port::from(9000),
            nic_list: vec![
                Nic {
                    addr: Ipv4Addr::new(127, 0, 0, 1),
                    nic_type: NicType::Ethernet,
                    alias: "Ethernet".into(),
                },
                Nic {
                    addr: Ipv4Addr::new(127, 0, 0, 1),
                    nic_type: NicType::Rdma,
                    alias: "Infiniband".into(),
                },
            ],
        },
        Some("ACKID".into()),
    )
    .await;

    let meta_list: GetNodesResp = n
        .request(GetNodes {
            node_type: NodeType::Meta,
        })
        .await;

    assert_eq!(1, meta_list.nodes.len());
    assert_eq!(NodeID::from(123), meta_list.nodes[0].num_id);
}

#[net_test("connAuthFile=/connAuthFile")]
async fn authenticate() {
    let n = NodeDummy::run(Port::from(9000)).await;

    let auth = AuthenticationSecret::from_bytes("shared_secret\n".as_bytes());

    n.send(AuthenticateChannel { auth_secret: auth }).await;

    let _: GetNodesResp = n
        .request(GetNodes {
            node_type: NodeType::Meta,
        })
        .await;
}
