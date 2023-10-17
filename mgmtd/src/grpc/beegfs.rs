use super::*;
use crate::types::{NicType, NodeType};
use anyhow::{bail, Result};
use itertools::Itertools;
use shared::error_chain;
use sql_check::sql;
use std::net::Ipv4Addr;
use std::str::FromStr;

/// gRPC server implementation
#[derive(Debug)]
pub struct ManagementService {
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

        let nodes = self
            .ctx
            .db
            .op(move |tx| {
                let mut node_stmt = tx.prepare_cached(sql!(
                    "SELECT node_uid, node_id, node_type, alias, port FROM all_nodes_v"
                ))?;

                let mut nics_stmt = tx.prepare_cached(sql!(
                    "SELECT addr, name, nic_type FROM node_nics WHERE node_uid = ?1"
                ))?;

                let mut res = node_stmt.query([])?;

                let mut nodes = vec![];
                while let Some(row) = res.next()? {
                    let uid = row.get(0)?;

                    let r#type = NodeType::from_str(row.get_ref(2)?.as_str()?)?;
                    let r#type = match r#type {
                        NodeType::Meta => Type::Meta,
                        NodeType::Storage => Type::Storage,
                        NodeType::Client => Type::Client,
                    } as i32;

                    let nics = if req.get_ref().include_nics {
                        nics_stmt
                            .query_map([uid], |row| {
                                Ok(Nic {
                                    addr: Ipv4Addr::from(row.get::<_, [u8; 4]>(0)?).to_string(),
                                    name: row.get(1)?,
                                    r#type: match row.get::<_, NicType>(2)? {
                                        NicType::Ethernet => nic::Type::Ethernet,
                                        NicType::Sdp => nic::Type::Sdp,
                                        NicType::Rdma => nic::Type::Rdma,
                                    } as i32,
                                })
                            })?
                            .try_collect()?
                    } else {
                        vec![]
                    };

                    nodes.push(Node {
                        uid,
                        node_id: row.get(1)?,
                        r#type,
                        alias: row.get(3)?,
                        beemsg_port: row.get(4)?,
                        nics,
                    })
                }

                Ok(nodes)
            })
            .await
            .map_err(|e| {
                Status::new(Code::Internal, error_chain!(e, "Getting node list failed"))
            })?;

        Ok(Response::new(GetNodeListResp { nodes }))
    }

    /// Return a list of known NICs for the requested node
    async fn get_node_info(
        &self,
        req: Request<GetNodeInfoReq>,
    ) -> Result<Response<GetNodeInfoResp>, Status> {
        let req = req.into_inner();

        let nics = self
            .ctx
            .db
            .op(move |tx| {
                let node_uid = match req.key {
                    Some(get_node_info_req::Key::Uid(node_uid)) => {
                        if !db::node::is_uid(tx, node_uid)? {
                            bail!(TypedError::value_not_found("UID", node_uid));
                        }
                        node_uid
                    }
                    Some(get_node_info_req::Key::Alias(alias)) => {
                        match db::entity::get_uid(tx, &alias)? {
                            Some(node_uid) => node_uid,
                            None => bail!(TypedError::value_not_found("alias", alias)),
                        }
                    }
                    None => {
                        bail!("Neither a UID nor an alias was given.");
                    }
                };

                let mut stmt = tx.prepare_cached(sql!(
                    "SELECT addr, name, nic_type FROM node_nics WHERE node_uid = ?"
                ))?;

                let res = stmt
                    .query_map([node_uid], |row| {
                        Ok(Nic {
                            addr: Ipv4Addr::from(row.get::<_, [u8; 4]>(0)?).to_string(),
                            name: row.get(1)?,
                            r#type: match row.get::<_, NicType>(2)? {
                                NicType::Ethernet => nic::Type::Ethernet,
                                NicType::Sdp => nic::Type::Sdp,
                                NicType::Rdma => nic::Type::Rdma,
                            } as i32,
                        })
                    })?
                    .try_collect()?;

                Ok(res)
            })
            .await
            .map_err(|err| match err.downcast_ref() {
                Some(TypedError::ValueNotFound { .. }) => {
                    Status::new(Code::NotFound, err.to_string())
                }
                _ => Status::new(Code::Internal, error_chain!(err, "Getting nics failed")),
            })?;

        Ok(Response::new(GetNodeInfoResp { nics }))
    }
}
