//! gRPC server and handlers

use crate::context::Context;
use anyhow::{Context as AContext, Result};
use protobuf::management::*;
use rusqlite::params;
use shared::error_chain;
use shared::shutdown::Shutdown;
use shared::types::EntityUID;
use sqlite::{ConnectionExt, TransactionExt};
use sqlite_check::sql;
use std::net::SocketAddr;
use tonic::transport::{Identity, Server, ServerTlsConfig};
use tonic::{Code, Request, Response, Status};

mod buddy_group;
mod misc;
mod node;
mod storage_pool;
mod target;

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
            .add_service(management_server::ManagementServer::new(
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
impl management_server::Management for ManagementService {
    async fn get_nodes(
        &self,
        req: Request<GetNodesRequest>,
    ) -> Result<Response<GetNodesResponse>, Status> {
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

    async fn get_targets(
        &self,
        req: Request<GetTargetsRequest>,
    ) -> Result<Response<GetTargetsResponse>, Status> {
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

    async fn get_buddy_groups(
        &self,
        req: Request<GetBuddyGroupsRequest>,
    ) -> Result<Response<GetBuddyGroupsResponse>, Status> {
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

    async fn get_storage_pools(
        &self,
        req: Request<GetStoragePoolsRequest>,
    ) -> Result<Response<GetStoragePoolsResponse>, Status> {
        let res = storage_pool::get(&self.ctx, req.into_inner()).await;

        match res {
            Ok(res) => Ok(Response::new(res)),
            Err(err) => {
                let msg = error_chain!(err, "Getting storage pools failed");
                log::error!("{msg}");
                Err(Status::new(Code::Internal, msg))
            }
        }
    }

    async fn set_alias(
        &self,
        req: Request<SetAliasRequest>,
    ) -> Result<Response<SetAliasResponse>, Status> {
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
}
