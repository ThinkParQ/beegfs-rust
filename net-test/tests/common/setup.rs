#![allow(dead_code)]

pub use net_test::node_dummy::*;
pub use net_test::*;
pub use shared::msg::*;
pub use shared::types::*;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::ops::Deref;

pub struct Node {
    pub node_handle: NodeDummy,
    pub num_id: NodeID,
}

impl Node {
    pub async fn setup(port: Port, node_type: NodeType) -> Self {
        let n = NodeDummy::run(port).await;

        let resp: RegisterNodeResp = n
            .request(RegisterNode {
                instance_version: 0,
                nic_list_version: 0,
                node_type,
                node_alias: format!("node_p{port}").as_str().into(),
                node_num_id: NodeID::ZERO,
                root_num_id: NodeID::ZERO,
                is_root_mirrored: false,
                port,
                port_tcp_unused: port,
                nic_list: vec![Nic {
                    addr: Ipv4Addr::new(127, 0, 0, 1),
                    nic_type: NicType::Ethernet,
                    alias: "Ethernet".into(),
                }],
            })
            .await;

        Self {
            node_handle: n,
            num_id: resp.node_num_id,
        }
    }
}

impl Deref for Node {
    type Target = NodeDummy;

    fn deref(&self) -> &Self::Target {
        &self.node_handle
    }
}

pub struct Target {
    pub id: TargetID,
    pub on_node: NodeID,
}

impl Target {
    pub async fn setup(on: &Node, alias: &str) -> Self {
        let target: RegisterStorageTargetResp = on
            .request(RegisterStorageTarget {
                alias: alias.into(),
                id: TargetID::ZERO,
            })
            .await;

        assert_ne!(TargetID::ZERO, target.id);

        // should be sent from ctl, but in reality doesn't matter
        let r: MapTargetsResp = on
            .request(MapTargets {
                targets: HashMap::from([(target.id, StoragePoolID::from(1))]),
                node_num_id: on.num_id,
                ack_id: "".into(),
            })
            .await;

        assert_eq!(r.results.iter().next().unwrap().1, &OpsErr::SUCCESS);

        Self {
            id: target.id,
            on_node: on.num_id,
        }
    }
}

pub struct StoragePool {
    pub id: StoragePoolID,
}

impl StoragePool {
    pub async fn setup(targets: &[TargetID], ctl: &NodeDummy, alias: &str) -> Self {
        let r: AddStoragePoolResp = ctl
            .request(AddStoragePool {
                id: StoragePoolID::ZERO,
                alias: alias.into(),
                move_target_ids: targets.to_vec(),
                move_buddy_group_ids: vec![],
            })
            .await;

        assert_eq!(OpsErr::SUCCESS, r.result);
        assert_ne!(StoragePoolID::ZERO, r.pool_id);

        Self { id: r.pool_id }
    }
}

pub struct DefaultNodes {
    pub ctl: NodeDummy,
    pub meta: [Node; 2],
    pub storage: [Node; 2],
    pub client: [Node; 2],
}

impl DefaultNodes {
    pub async fn setup() -> Self {
        Self {
            ctl: NodeDummy::run(Port::from(12345)).await,
            meta: [
                Node::setup(Port::from(10000), NodeType::Meta).await,
                Node::setup(Port::from(10001), NodeType::Meta).await,
            ],
            storage: [
                Node::setup(Port::from(10010), NodeType::Storage).await,
                Node::setup(Port::from(10011), NodeType::Storage).await,
            ],
            client: [
                Node::setup(Port::from(10020), NodeType::Client).await,
                Node::setup(Port::from(10021), NodeType::Client).await,
            ],
        }
    }
}
