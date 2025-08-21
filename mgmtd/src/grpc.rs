//! gRPC server and handlers

use crate::bee_msg::notify_nodes;
use crate::context::Context;
use crate::db;
use crate::license::LicensedFeature;
use crate::types::{ResolveEntityId, SqliteEnumExt};
use anyhow::{Context as AContext, Result, anyhow, bail};
use protobuf::{beegfs as pb, management as pm};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use shared::grpc::*;
use shared::impl_grpc_handler;
use shared::run_state::RunStateHandle;
use shared::types::*;
use sqlite::{TransactionExt, check_affected_rows};
use sqlite_check::sql;
use std::fmt::Debug;
use std::future::Future;
use std::net::{SocketAddr, TcpListener};
use std::pin::Pin;
use tonic::transport::{Identity, Server, ServerTlsConfig};
use tonic::{Code, Request, Response, Status};

mod buddy_group;
mod license;
mod misc;
mod node;
mod pool;
mod quota;
mod target;

/// Management gRPC service implementation struct
#[derive(Debug)]
pub(crate) struct ManagementService {
    pub ctx: Context,
}

/// Implementation of the management gRPC service. Use the shared::impl_grpc_handler! macro to
/// implement each function.
///
/// However, if a function should be implemented manually using an async fn, re-add the
/// #[tonic::async_trait] macro (or it will not work).
impl pm::management_server::Management for ManagementService {
    // Example: Implement pm::management_server::Management::set_alias using the impl_grpc_handler
    // macro
    impl_grpc_handler! {
        // <the function to implement (as defined by the trait)> => <the actual, custom handler function to call>,
        set_alias => misc::set_alias,
        // <request message passed to the fn impl (as defined by the trait)> => <response message,
        // returned by the fn impl (as defined by the trait)>,
        pm::SetAliasRequest => pm::SetAliasResponse,
        // <context string for logged errors>
        "Set alias"
    }

    impl_grpc_handler! {
        get_nodes => node::get,
        pm::GetNodesRequest => pm::GetNodesResponse,
        "Get nodes"
    }
    impl_grpc_handler! {
        delete_node => node::delete,
        pm::DeleteNodeRequest => pm::DeleteNodeResponse,
        "Delete node"
    }

    impl_grpc_handler! {
        get_targets => target::get,
        pm::GetTargetsRequest => pm::GetTargetsResponse,
        "Get targets"
    }
    impl_grpc_handler! {
        delete_target => target::delete,
        pm::DeleteTargetRequest => pm::DeleteTargetResponse,
        "Delete target"
    }
    impl_grpc_handler! {
        set_target_state => target::set_state,
        pm::SetTargetStateRequest => pm::SetTargetStateResponse,
        "Set target state"
    }

    impl_grpc_handler! {
        get_pools => pool::get,
        pm::GetPoolsRequest => pm::GetPoolsResponse,
        "Get pools"
    }
    impl_grpc_handler! {
        create_pool => pool::create,
        pm::CreatePoolRequest => pm::CreatePoolResponse,
        "Create pool"
    }
    impl_grpc_handler! {
        assign_pool => pool::assign,
        pm::AssignPoolRequest => pm::AssignPoolResponse,
        "Assign pool"
    }
    impl_grpc_handler! {
        delete_pool => pool::delete,
        pm::DeletePoolRequest => pm::DeletePoolResponse,
        "Delete pool"
    }

    impl_grpc_handler! {
        get_buddy_groups => buddy_group::get,
        pm::GetBuddyGroupsRequest => pm::GetBuddyGroupsResponse,
        "Get buddy groups"
    }
    impl_grpc_handler! {
        create_buddy_group => buddy_group::create,
        pm::CreateBuddyGroupRequest => pm::CreateBuddyGroupResponse,
        "Create buddy group"
    }
    impl_grpc_handler! {
        delete_buddy_group => buddy_group::delete,
        pm::DeleteBuddyGroupRequest => pm::DeleteBuddyGroupResponse,
        "Delete buddy group"
    }
    impl_grpc_handler! {
        mirror_root_inode => buddy_group::mirror_root_inode,
        pm::MirrorRootInodeRequest => pm::MirrorRootInodeResponse,
        "Mirror root inode"
    }
    impl_grpc_handler! {
        start_resync => buddy_group::start_resync,
        pm::StartResyncRequest => pm::StartResyncResponse,
        "Start resync"
    }

    impl_grpc_handler! {
        set_default_quota_limits => quota::set_default_quota_limits,
        pm::SetDefaultQuotaLimitsRequest => pm::SetDefaultQuotaLimitsResponse,
        "Set default quota limits"
    }
    impl_grpc_handler! {
        set_quota_limits => quota::set_quota_limits,
        pm::SetQuotaLimitsRequest => pm::SetQuotaLimitsResponse,
        "Set quota limits"
    }
    impl_grpc_handler! {
        get_quota_limits => quota::get_quota_limits,
        pm::GetQuotaLimitsRequest => STREAM(GetQuotaLimitsStream, pm::GetQuotaLimitsResponse),
        "Get quota limits"
    }
    impl_grpc_handler! {
        get_quota_usage => quota::get_quota_usage,
        pm::GetQuotaUsageRequest => STREAM(GetQuotaUsageStream, pm::GetQuotaUsageResponse),
        "Get quota usage"
    }

    impl_grpc_handler! {
        get_license => license::get,
        pm::GetLicenseRequest => pm::GetLicenseResponse,
        "Get license"
    }
}

/// Serve gRPC requests on the `grpc_port` extracted from the config
pub(crate) fn serve(ctx: Context, mut shutdown: RunStateHandle) -> Result<()> {
    let builder = Server::builder();

    // If gRPC TLS is enabled, configure the server accordingly
    let mut builder = if !ctx.info.user_config.tls_disable {
        let tls_cert = std::fs::read(&ctx.info.user_config.tls_cert_file).with_context(|| {
            format!(
                "Could not read TLS certificate file {:?}",
                &ctx.info.user_config.tls_cert_file
            )
        })?;
        let tls_key = std::fs::read(&ctx.info.user_config.tls_key_file).with_context(|| {
            format!(
                "Could not read TLS key file {:?}",
                &ctx.info.user_config.tls_key_file
            )
        })?;

        builder
            .tls_config(ServerTlsConfig::new().identity(Identity::from_pem(tls_cert, tls_key)))?
    } else {
        log::warn!("gRPC server running with TLS disabled");
        builder
    };

    let ctx2 = ctx.clone();
    let service = pm::management_server::ManagementServer::with_interceptor(
        ManagementService { ctx: ctx.clone() },
        move |req: Request<()>| {
            // If authentication is enabled, require the secret passed with every request
            if let Some(required_secret) = ctx2.info.auth_secret {
                let check = || -> Result<()> {
                    let Some(request_secret) = req.metadata().get("auth-secret") else {
                        bail!("Request requires authentication but no secret was provided")
                    };

                    let request_secret = AuthSecret::try_from_bytes(request_secret.as_bytes())?;

                    if request_secret != required_secret {
                        bail!("Request requires authentication but provided secret doesn't match",);
                    }

                    Ok(())
                };

                if let Err(err) = check() {
                    return Err(Status::unauthenticated(err.to_string()));
                }
            }

            Ok(req)
        },
    );

    let mut serve_addr = SocketAddr::new("::".parse()?, ctx.info.user_config.grpc_port);

    // Test for IPv6 available, fall back to IPv4 sockets if not
    match TcpListener::bind(serve_addr) {
        Ok(_) => {}
        Err(err) if err.raw_os_error() == Some(libc::EAFNOSUPPORT) => {
            log::debug!("gRPC: IPv6 not available, falling back to IPv4 sockets");
            serve_addr = SocketAddr::new("0.0.0.0".parse()?, ctx.info.user_config.grpc_port);
        }
        Err(err) => {
            anyhow::bail!(err);
        }
    }

    log::info!("Serving gRPC requests on {serve_addr}");

    tokio::spawn(async move {
        builder
            .add_service(service)
            // Provide our shutdown handle to automatically shutdown the server gracefully when
            // requested
            .serve_with_shutdown(serve_addr, shutdown.wait_for_shutdown())
            .await
            .ok();
    });

    Ok(())
}

/// Checks if the given license feature is enabled or fails with "Unauthenticated" if not
fn needs_license(ctx: &Context, feature: LicensedFeature) -> Result<()> {
    ctx.license
        .verify_feature(feature)
        .status_code(Code::Unauthenticated)
}

/// Checks if the management is in pre shutdown state
fn fail_on_pre_shutdown(ctx: &Context) -> Result<()> {
    if ctx.run_state.pre_shutdown() {
        return Err(anyhow!("Management is shutting down")).status_code(Code::Unavailable);
    }

    Ok(())
}
