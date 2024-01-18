use super::*;
use crate::db;
use crate::types::{NicType, NodeType};
use anyhow::Result;
use shared::error_chain;

/// gRPC server implementation
#[derive(Debug)]
pub(crate) struct ManagementService {
    pub ctx: Context,
}

/// gRPC server implementation
#[tonic::async_trait]
impl management_server::Management for ManagementService {
    /// Return a list of all registered nodes
    async fn get_node_list(
        &self,
        req: Request<GetNodeListReq>,
    ) -> Result<Response<GetNodeListResp>, Status> {
        use get_node_list_resp::node::Type;
        use get_node_list_resp::Node;

        let (nodes, nics) = self
            .ctx
            .db
            .op(move |tx| {
                let nodes = db::node::get_all(tx)?;
                let nics = if req.get_ref().include_nics {
                    Some(db::node_nic::get_all(tx)?)
                } else {
                    None
                };

                Ok((nodes, nics))
            })
            .await
            .map_err(|e| {
                Status::new(Code::Internal, error_chain!(e, "Getting node list failed"))
            })?;

        let res = nodes
            .into_iter()
            .map(|node| {
                let r#type = match node.node_type {
                    NodeType::Meta => Type::Meta,
                    NodeType::Storage => Type::Storage,
                    NodeType::Client => Type::Client,
                } as i32;

                let nics = if let Some(nics) = nics.clone() {
                    nics.iter()
                        .filter(|nic| nic.node_uid == node.uid)
                        .map(|nic| Nic {
                            addr: nic.addr.to_string(),
                            name: nic.name.to_string(),
                            r#type: match nic.nic_type {
                                NicType::Ethernet => nic::Type::Ethernet,
                                NicType::Sdp => nic::Type::Sdp,
                                NicType::Rdma => nic::Type::Rdma,
                            } as i32,
                        })
                        .collect()
                } else {
                    vec![]
                };

                Node {
                    uid: node.uid,
                    node_id: node.id.into(),
                    r#type,
                    alias: node.alias,
                    beemsg_port: node.port.into(),
                    nics,
                }
            })
            .collect();

        Ok(Response::new(GetNodeListResp {
            nodes: res,
            meta_root_node_uid: -1,
        }))
    }
}
