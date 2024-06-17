//! gRPC server and handlers

use crate::bee_msg::notify_nodes;
use crate::context::Context;
use crate::db;
use crate::types::{ResolveEntityId, SqliteExt};
use anyhow::{bail, Context as AContext, Result};
use protobuf::{beegfs as pb, management as pm};
use rusqlite::{params, Transaction};
use shared::error_chain;
use shared::shutdown::Shutdown;
use shared::types::*;
use sqlite::{check_affected_rows, ConnectionExt, TransactionExt};
use sqlite_check::sql;
use std::net::SocketAddr;
use tonic::transport::{Identity, Server, ServerTlsConfig};
use tonic::{Code, Request, Response, Status};

mod buddy_group;
mod misc;
mod node;
mod pool;
mod target;

/// Unwraps an optional proto message field . If `None`, errors out providing the fields name in the
/// error message.
///
/// Meant for unwrapping optional protobuf fields that are actually mandatory.
pub(crate) fn required_field<T>(f: Option<T>) -> Result<T> {
    f.ok_or_else(|| ::anyhow::anyhow!("missing required {} field", std::any::type_name::<T>()))
}

/// Serve gRPC requests on the `grpc_port` extracted from the config
pub(crate) fn serve(ctx: Context, mut shutdown: Shutdown) -> Result<()> {
    let builder = Server::builder();

    // If gRPC TLS is enabled, configure the server accordingly
    let mut builder = if ctx.info.user_config.grpc_tls_enable {
        let tls_cert = std::fs::read(&ctx.info.user_config.tls_cert_file)
            .context("Could not read certificate file")?;
        let tls_key =
            std::fs::read(&ctx.info.user_config.tls_key_file).context("Could not read key file")?;

        builder
            .tls_config(ServerTlsConfig::new().identity(Identity::from_pem(tls_cert, tls_key)))?
    } else {
        log::warn!("gRPC server running with TLS disabled");
        builder
    };

    let serve_addr = SocketAddr::new("0.0.0.0".parse()?, ctx.info.user_config.grpc_port);

    log::info!("Serving gRPC requests on {serve_addr}");

    tokio::spawn(async move {
        builder
            .add_service(pm::management_server::ManagementServer::new(
                ManagementService { ctx },
            ))
            // Provide our shutdown handle to automatically shutdown the server gracefully when
            // requested
            .serve_with_shutdown(serve_addr, shutdown.wait())
            .await
            .ok();
    });

    Ok(())
}

/// gRPC server implementation
#[derive(Debug)]
pub(crate) struct ManagementService {
    pub ctx: Context,
}

/// gRPC server implementation
#[tonic::async_trait]
impl pm::management_server::Management for ManagementService {
    async fn set_alias(
        &self,
        req: Request<pm::SetAliasRequest>,
    ) -> Result<Response<pm::SetAliasResponse>, Status> {
        let res = misc::set_alias(&self.ctx, req.into_inner()).await;

        match res {
            Ok(res) => Ok(Response::new(res)),
            Err(err) => {
                let msg = error_chain!(err, "Setting alias failed");
                log::error!("{msg}");
                Err(Status::new(Code::Internal, msg))
            }
        }
    }

    async fn get_nodes(
        &self,
        req: Request<pm::GetNodesRequest>,
    ) -> Result<Response<pm::GetNodesResponse>, Status> {
        let res = node::get(&self.ctx, req.into_inner()).await;

        match res {
            Ok(res) => Ok(Response::new(res)),
            Err(err) => {
                let msg = error_chain!(err, "Getting nodes failed");
                log::error!("{msg}");
                Err(Status::new(Code::Internal, msg))
            }
        }
    }

    async fn delete_node(
        &self,
        req: Request<pm::DeleteNodeRequest>,
    ) -> Result<Response<pm::DeleteNodeResponse>, Status> {
        let res = node::delete(&self.ctx, req.into_inner()).await;

        match res {
            Ok(res) => Ok(Response::new(res)),
            Err(err) => {
                let msg = error_chain!(err, "Deleting node failed");
                log::error!("{msg}");
                Err(Status::new(Code::Internal, msg))
            }
        }
    }

    async fn get_targets(
        &self,
        req: Request<pm::GetTargetsRequest>,
    ) -> Result<Response<pm::GetTargetsResponse>, Status> {
        let res = target::get(&self.ctx, req.into_inner()).await;

        match res {
            Ok(res) => Ok(Response::new(res)),
            Err(err) => {
                let msg = error_chain!(err, "Getting targets failed");
                log::error!("{msg}");
                Err(Status::new(Code::Internal, msg))
            }
        }
    }

    async fn delete_target(
        &self,
        req: Request<pm::DeleteTargetRequest>,
    ) -> Result<Response<pm::DeleteTargetResponse>, Status> {
        let res = target::delete(&self.ctx, req.into_inner()).await;

        match res {
            Ok(res) => Ok(Response::new(res)),
            Err(err) => {
                let msg = error_chain!(err, "Deleting target failed");
                log::error!("{msg}");
                Err(Status::new(Code::Internal, msg))
            }
        }
    }

    async fn get_pools(
        &self,
        req: Request<pm::GetPoolsRequest>,
    ) -> Result<Response<pm::GetPoolsResponse>, Status> {
        let res = pool::get(&self.ctx, req.into_inner()).await;

        match res {
            Ok(res) => Ok(Response::new(res)),
            Err(err) => {
                let msg = error_chain!(err, "Getting storage pools failed");
                log::error!("{msg}");
                Err(Status::new(Code::Internal, msg))
            }
        }
    }

    async fn create_pool(
        &self,
        req: Request<pm::CreatePoolRequest>,
    ) -> Result<Response<pm::CreatePoolResponse>, Status> {
        let res = pool::create(&self.ctx, req.into_inner()).await;

        match res {
            Ok(res) => Ok(Response::new(res)),
            Err(err) => {
                let msg = error_chain!(err, "Creating storage pool failed");
                log::error!("{msg}");
                Err(Status::new(Code::Internal, msg))
            }
        }
    }

    async fn assign_pool(
        &self,
        req: Request<pm::AssignPoolRequest>,
    ) -> Result<Response<pm::AssignPoolResponse>, Status> {
        let res = pool::assign(&self.ctx, req.into_inner()).await;

        match res {
            Ok(res) => Ok(Response::new(res)),
            Err(err) => {
                let msg = error_chain!(err, "Moving targets to storage pool failed");
                log::error!("{msg}");
                Err(Status::new(Code::Internal, msg))
            }
        }
    }

    async fn delete_pool(
        &self,
        req: Request<pm::DeletePoolRequest>,
    ) -> Result<Response<pm::DeletePoolResponse>, Status> {
        let res = pool::delete(&self.ctx, req.into_inner()).await;

        match res {
            Ok(res) => Ok(Response::new(res)),
            Err(err) => {
                let msg = error_chain!(err, "Deleting storage pool failed");
                log::error!("{msg}");
                Err(Status::new(Code::Internal, msg))
            }
        }
    }

    async fn get_buddy_groups(
        &self,
        req: Request<pm::GetBuddyGroupsRequest>,
    ) -> Result<Response<pm::GetBuddyGroupsResponse>, Status> {
        let res = buddy_group::get(&self.ctx, req.into_inner()).await;

        match res {
            Ok(res) => Ok(Response::new(res)),
            Err(err) => {
                let msg = error_chain!(err, "Getting buddy groups failed");
                log::error!("{msg}");
                Err(Status::new(Code::Internal, msg))
            }
        }
    }

    async fn create_buddy_group(
        &self,
        req: Request<pm::CreateBuddyGroupRequest>,
    ) -> Result<Response<pm::CreateBuddyGroupResponse>, Status> {
        let res = buddy_group::create(&self.ctx, req.into_inner()).await;

        match res {
            Ok(res) => Ok(Response::new(res)),
            Err(err) => {
                let msg = error_chain!(err, "Creating storage buddy_group failed");
                log::error!("{msg}");
                Err(Status::new(Code::Internal, msg))
            }
        }
    }

    async fn delete_buddy_group(
        &self,
        req: Request<pm::DeleteBuddyGroupRequest>,
    ) -> Result<Response<pm::DeleteBuddyGroupResponse>, Status> {
        let res = buddy_group::delete(&self.ctx, req.into_inner()).await;

        match res {
            Ok(res) => Ok(Response::new(res)),
            Err(err) => {
                let msg = error_chain!(err, "Deleting storage buddy_group failed");
                log::error!("{msg}");
                Err(Status::new(Code::Internal, msg))
            }
        }
    }
}
