mod management;

use crate::context::Context;
use crate::db;
use crate::db::TypedError;
use anyhow::Result;
use management::ManagementService;
use pb::beegfs::management::*;
use shared::shutdown::Shutdown;
use std::net::SocketAddr;
use tonic::transport::Server;
use tonic::{Code, Request, Response, Status};

/// Serve gRPC requests on the `grpc_port` extracted from the config
pub async fn serve(ctx: Context, mut shutdown: Shutdown) -> Result<()> {
    let port = ctx.info.config.grpc_port;

    Server::builder()
        .add_service(management_server::ManagementServer::new(
            ManagementService { ctx },
        ))
        .serve_with_shutdown(SocketAddr::new("0.0.0.0".parse()?, port), shutdown.wait())
        .await?;

    Ok(())
}
