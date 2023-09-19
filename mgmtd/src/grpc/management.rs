use super::*;
use anyhow::{bail, Result};
use pb::beegfs::management::get_nics_for_node_resp::Nic;
use pb::beegfs::management::get_nodes_resp::node::NodeType;
use pb::beegfs::management::get_nodes_resp::Node;
use shared::error_chain;
use sql_check::sql;

/// gRPC server implementation
#[derive(Debug)]
pub struct ManagementService {
    pub ctx: Context,
}

/// gRPC server implementation
#[tonic::async_trait]
impl management_server::Management for ManagementService {
    /// Return a list of all registered nodes
    async fn get_nodes(
        &self,
        _req: Request<GetNodesReq>,
    ) -> Result<Response<GetNodesResp>, Status> {
        let nodes = self
            .ctx
            .db
            .op(move |tx| {
                let mut stmt = tx
                    .prepare_cached(sql!("SELECT node_uid, node_id, node_type FROM all_nodes_v"))?;

                let res = stmt
                    .query_map([], |row| {
                        let node_type = row.get_ref(2)?.as_str()?.to_uppercase();
                        let node_type = NodeType::from_str_name(&node_type).unwrap() as i32;

                        Ok(Node {
                            node_uid: row.get(0)?,
                            node_id: row.get(1)?,
                            node_type,
                        })
                    })?
                    .try_collect()?;

                Ok(res)
            })
            .await
            .map_err(|e| {
                Status::new(Code::Internal, error_chain!(e, "Getting node list failed"))
            })?;

        Ok(Response::new(GetNodesResp { nodes }))
    }

    /// Return a list of known NICs for the requested node
    async fn get_nics_for_node(
        &self,
        req: Request<GetNicsForNodeReq>,
    ) -> Result<Response<GetNicsForNodeResp>, Status> {
        let node_uid = req.get_ref().node_uid;

        let nics = self
            .ctx
            .db
            .op(move |tx| {
                if !db::node::is_uid(tx, node_uid)? {
                    bail!(TypedError::value_not_found("node UID", node_uid));
                }

                let mut stmt = tx.prepare_cached(sql!(
                    "SELECT nic_uid, addr, name, nic_type FROM node_nics WHERE node_uid = ?"
                ))?;

                let res = stmt
                    .query_map([node_uid], |row| {
                        Ok(Nic {
                            nic_uid: row.get(0)?,
                            addr: row.get(1)?,
                            name: row.get(2)?,
                            nic_type: row.get(3)?,
                        })
                    })?
                    .try_collect()?;

                Ok(res)
            })
            .await
            .map_err(|err| match err.downcast_ref() {
                Some(TypedError::ValueNotFound { .. }) => {
                    Status::new(Code::NotFound, "NodeUID not found")
                }
                _ => Status::new(Code::Internal, error_chain!(err, "Getting nics failed")),
            })?;

        Ok(Response::new(GetNicsForNodeResp { nics }))
    }
}
