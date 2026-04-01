use super::*;
use db::misc::MetaRoot;
use db::node_nic::map_bee_msg_nics;
use shared::bee_msg::node::*;

impl HandleWithResponse for GetNodes {
    type Response = GetNodesResp;

    async fn handle(self, app: &impl App, _req: &mut impl Request) -> Result<Self::Response> {
        let (nodes, meta_root) = app
            .read_tx(move |tx| {
                let node_type = self.node_type;
                let nics = db::node_nic::get_with_type(tx, node_type)?;

                let nodes: Vec<_> = tx.query_map_collect(
                    sql!(
                        "SELECT node_uid, port, alias, node_id FROM nodes_ext
                        WHERE node_type = ?1 ORDER BY node_id ASC"
                    ),
                    [node_type.sql_variant()],
                    |row| {
                        let uid = row.get(0)?;
                        let port = row.get(1)?;
                        Ok(Node {
                            alias: row.get::<_, String>(2)?.into_bytes(),
                            nic_list: map_bee_msg_nics(
                                nics.iter().filter(|e| e.node_uid == uid).cloned(),
                            )
                            .collect(),
                            num_id: row.get(3)?,
                            port,
                            _unused_tcp_port: port,
                            node_type,
                        })
                    },
                )?;

                Ok((
                    nodes,
                    match self.node_type {
                        shared::types::NodeType::Meta => db::misc::get_meta_root(tx)?,
                        _ => MetaRoot::Unknown,
                    },
                ))
            })
            .await?;

        let resp = GetNodesResp {
            nodes,
            root_num_id: match meta_root {
                MetaRoot::Unknown => 0,
                MetaRoot::Normal(node_id, _) => node_id,
                MetaRoot::Mirrored(ref group_id) => group_id.raw().into(),
            },
            is_root_mirrored: match meta_root {
                MetaRoot::Unknown => 0,
                MetaRoot::Normal(_, _) => 0,
                MetaRoot::Mirrored(_) => 1,
            },
        };

        Ok(resp)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::app::test::*;

    #[tokio::test]
    async fn get_nodes() {
        let app = TestApp::new().await;
        let mut req = TestRequest::new(GetNodes::ID);

        let resp = GetNodes {
            node_type: NodeType::Meta,
        }
        .handle(&app, &mut req)
        .await
        .unwrap();

        assert_eq_db!(app, "SELECT COUNT(*) FROM meta_nodes", [], resp.nodes.len());
        assert_eq!(resp.root_num_id, 1);
        assert_eq!(resp.is_root_mirrored, 0);
    }
}
