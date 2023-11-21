mod beegfs;

use crate::context::Context;
use anyhow::{Context as AContext, Result};
use beegfs::ManagementService;
use pb::beegfs::beegfs::*;
use shared::shutdown::Shutdown;
use std::net::SocketAddr;
use tonic::transport::{Identity, Server, ServerTlsConfig};
use tonic::{Code, Request, Response, Status};

/// Serve gRPC requests on the `grpc_port` extracted from the config
pub fn serve(ctx: Context, mut shutdown: Shutdown) -> Result<()> {
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
