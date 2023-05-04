use crate::{db, MgmtdPool};
use anyhow::Result;
use shared::conn::PeerID;
use shared::msg::Msg;
use shared::*;

pub(crate) async fn request_tcp_by_type<M: Msg, R: Msg>(
    conn_pool: &MgmtdPool,
    db: &db::Handle,
    node_type: NodeTypeServer,
    msg: M,
) -> Result<Vec<R>> {
    let nodes = db
        .execute(move |tx| db::nodes::with_type(tx, node_type.into()))
        .await?;

    let mut responses = vec![];
    for node in nodes {
        match conn_pool.request(PeerID::Node(node.uid), &msg).await {
            Ok(resp) => responses.push(resp),
            Err(err) => log::warn!(
                "Failed to send message to node {:?}:\n{:?}",
                node.alias,
                err
            ),
        }
    }

    Ok(responses)
}

pub(crate) async fn notify_nodes<M: Notification<'static> + Sync>(
    conn_pool: &MgmtdPool,
    db: &db::Handle,
    msg: &M,
) {
    let res: Result<()> = try {
        for t in msg.notification_node_types() {
            let nodes = db.execute(move |tx| db::nodes::with_type(tx, *t)).await?;

            conn_pool
                .broadcast(nodes.into_iter().map(|e| PeerID::Node(e.uid)), msg)
                .await?;
        }
    };

    if let Err(err) = res {
        log::error!(
            "Could not broadcast notification of type {} to all nodes: {}",
            std::any::type_name::<M>(),
            err
        )
    }
}

pub(crate) trait Notification<'a>: Msg + Sync {
    fn notification_node_types(&self) -> &'a [NodeType];
}

macro_rules! impl_for {
    ($msg:path, $($node_types:ident),+) => {
        impl<'a> Notification<'a> for $msg {
            fn notification_node_types(&self) -> &'a [NodeType] {
                &[$(NodeType::$node_types),+]
            }
        }
    };
}

impl_for!(msg::MapTargets, Meta, Storage, Client);
impl_for!(msg::RefreshTargetStates, Meta, Storage, Client);
impl_for!(msg::RefreshCapacityPools, Meta);
impl_for!(msg::RefreshStoragePools, Meta, Storage);
impl_for!(msg::SetMirrorBuddyGroup, Meta, Storage, Client);

impl<'a> Notification<'a> for msg::Heartbeat {
    fn notification_node_types(&self) -> &'a [NodeType] {
        use NodeType::*;
        match self.node_type {
            Meta => &[Meta, Client],
            Storage => &[Meta, Client, Storage],
            Client => &[Meta],
            Management => &[],
        }
    }
}

impl<'a> Notification<'a> for msg::RemoveNode {
    fn notification_node_types(&self) -> &'a [NodeType] {
        use NodeType::*;
        match self.node_type {
            Meta => &[Meta, Client],
            Storage => &[Meta, Client, Storage],
            Client => &[],
            Management => &[],
        }
    }
}
