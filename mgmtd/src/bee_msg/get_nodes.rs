use super::*;
use db::misc::MetaRoot;
use db::node_nic::map_bee_msg_nics;
use shared::bee_msg::node::*;

impl HandleWithResponse for GetNodes {
    type Response = GetNodesResp;

    async fn handle(self, ctx: &Context, _req: &mut impl Request) -> Result<Self::Response> {
        let res = ctx
            .db
            .read_tx(move |tx| {
                let node_type = self.node_type;
                let res = (
                    db::node::get_with_type(tx, node_type)?,
                    db::node_nic::get_with_type(tx, node_type)?,
                    match self.node_type {
                        shared::types::NodeType::Meta => db::misc::get_meta_root(tx)?,
                        _ => MetaRoot::Unknown,
                    },
                );

                Ok(res)
            })
            .await?;

        let mut nodes: Vec<Node> = res
            .0
            .into_iter()
            .map(|n| Node {
                alias: n.alias.into_bytes(),
                num_id: n.id,
                nic_list: map_bee_msg_nics(res.1.iter().filter(|e| e.node_uid == n.uid).cloned())
                    .collect(),
                port: n.port,
                _unused_tcp_port: n.port,
                node_type: n.node_type,
            })
            .collect();

        nodes.sort_by(|a, b| a.num_id.cmp(&b.num_id));

        let resp = GetNodesResp {
            nodes,
            root_num_id: match res.2 {
                MetaRoot::Unknown => 0,
                MetaRoot::Normal(node_id, _) => node_id,
                MetaRoot::Mirrored(group_id) => group_id.into(),
            },
            is_root_mirrored: match res.2 {
                MetaRoot::Unknown => 0,
                MetaRoot::Normal(_, _) => 0,
                MetaRoot::Mirrored(_) => 1,
            },
        };

        Ok(resp)
    }
}
