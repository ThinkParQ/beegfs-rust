use super::*;
use db::misc::MetaRoot;

pub(super) async fn handle(
    msg: msg::GetNodes,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    match hnd
        .execute_db(move |tx| {
            let res = (
                db::nodes::with_type(tx, msg.node_type)?,
                db::node_nics::with_type(tx, msg.node_type)?,
                match msg.node_type {
                    NodeType::Meta => db::misc::get_meta_root(tx)?,
                    _ => MetaRoot::Unknown,
                },
            );

            Ok(res)
        })
        .await
    {
        Ok(res) => {
            chn.respond(&msg::GetNodesResp {
                nodes: res
                    .0
                    .into_iter()
                    .map(|n| msg::types::Node {
                        alias: n.alias,
                        num_id: n.id,
                        nic_list: res
                            .1
                            .iter()
                            .filter_map(|e| {
                                if e.node_uid == n.uid {
                                    Some(Nic {
                                        addr: e.addr,
                                        alias: e.alias.clone(),
                                        nic_type: e.nic_type,
                                    })
                                } else {
                                    None
                                }
                            })
                            .collect(),
                        port: n.port,
                        _unused_tcp_port: n.port,
                        node_type: n.node_type,
                    })
                    .collect(),
                root_num_id: match res.2 {
                    MetaRoot::Unknown => 0,
                    MetaRoot::Normal(_, node_id, _) => node_id.into(),
                    MetaRoot::Mirrored(buddy_group_id) => buddy_group_id.into(),
                },
                is_root_mirrored: match res.2 {
                    MetaRoot::Unknown => false,
                    MetaRoot::Normal(_, _, _) => false,
                    MetaRoot::Mirrored(_) => true,
                },
            })
            .await
        }
        Err(err) => {
            log::error!("Getting {} node list failed:\n{:?}", msg.node_type, err);

            chn.respond(&msg::GetNodesResp {
                nodes: vec![],
                root_num_id: 0,
                is_root_mirrored: false,
            })
            .await
        }
    }
}
