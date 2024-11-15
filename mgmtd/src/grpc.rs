//! gRPC server and handlers

use crate::app::*;
use crate::db;
use crate::license::LicensedFeature;
use crate::types::{ResolveEntityId, SqliteEnumExt};
use anyhow::{Context as AContext, Result, anyhow, bail};
use protobuf::{beegfs as pb, management as pm};
use rusqlite::{OptionalExtension, Row, Transaction, TransactionBehavior, named_params, params};
use shared::grpc::*;
use shared::impl_grpc_handler;
use shared::run_state::RunStateHandle;
use shared::types::*;
use sqlite::{TransactionExt, check_affected_rows};
use sqlite_check::sql;
use std::fmt::Debug;
use std::future::Future;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use tonic::transport::{Identity, Server, ServerTlsConfig};
use tonic::{Code, Request, Response, Status};

mod common;

mod assign_pool;
mod create_buddy_group;
mod create_pool;
mod delete_buddy_group;
mod delete_node;
mod delete_pool;
mod delete_target;
mod get_buddy_groups;
mod get_license;
mod get_nodes;
mod get_pools;
mod get_quota_limits;
mod get_quota_usage;
mod get_targets;
mod mirror_root_inode;
mod set_alias;
mod set_default_quota_limits;
mod set_quota_limits;
mod set_target_state;
mod start_resync;

/// Management gRPC service implementation struct
#[derive(Debug)]
pub(crate) struct ManagementService {
    pub app: RuntimeApp,
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
        // the function to implement (as defined by the trait) as well as the handler to call (must
        // be named the same and in a submodule named the same),
        set_alias,
        // <request message passed to the fn impl (as defined by the trait)> => <response message,
        // returned by the fn impl (as defined by the trait)>,
        pm::SetAliasRequest => pm::SetAliasResponse,
        // <context string for logged errors>
        "Set alias"
    }

    impl_grpc_handler! {
        get_nodes,
        pm::GetNodesRequest => pm::GetNodesResponse,
        "Get nodes"
    }
    impl_grpc_handler! {
        delete_node,
        pm::DeleteNodeRequest => pm::DeleteNodeResponse,
        "Delete node"
    }

    impl_grpc_handler! {
        get_targets,
        pm::GetTargetsRequest => pm::GetTargetsResponse,
        "Get targets"
    }
    impl_grpc_handler! {
        delete_target,
        pm::DeleteTargetRequest => pm::DeleteTargetResponse,
        "Delete target"
    }
    impl_grpc_handler! {
        set_target_state,
        pm::SetTargetStateRequest => pm::SetTargetStateResponse,
        "Set target state"
    }

    impl_grpc_handler! {
        get_pools,
        pm::GetPoolsRequest => pm::GetPoolsResponse,
        "Get pools"
    }
    impl_grpc_handler! {
        create_pool,
        pm::CreatePoolRequest => pm::CreatePoolResponse,
        "Create pool"
    }
    impl_grpc_handler! {
        assign_pool,
        pm::AssignPoolRequest => pm::AssignPoolResponse,
        "Assign pool"
    }
    impl_grpc_handler! {
        delete_pool,
        pm::DeletePoolRequest => pm::DeletePoolResponse,
        "Delete pool"
    }

    impl_grpc_handler! {
        get_buddy_groups,
        pm::GetBuddyGroupsRequest => pm::GetBuddyGroupsResponse,
        "Get buddy groups"
    }
    impl_grpc_handler! {
        create_buddy_group,
        pm::CreateBuddyGroupRequest => pm::CreateBuddyGroupResponse,
        "Create buddy group"
    }
    impl_grpc_handler! {
        delete_buddy_group,
        pm::DeleteBuddyGroupRequest => pm::DeleteBuddyGroupResponse,
        "Delete buddy group"
    }
    impl_grpc_handler! {
        mirror_root_inode,
        pm::MirrorRootInodeRequest => pm::MirrorRootInodeResponse,
        "Mirror root inode"
    }
    impl_grpc_handler! {
        start_resync,
        pm::StartResyncRequest => pm::StartResyncResponse,
        "Start resync"
    }

    impl_grpc_handler! {
        set_default_quota_limits,
        pm::SetDefaultQuotaLimitsRequest => pm::SetDefaultQuotaLimitsResponse,
        "Set default quota limits"
    }
    impl_grpc_handler! {
        set_quota_limits,
        pm::SetQuotaLimitsRequest => pm::SetQuotaLimitsResponse,
        "Set quota limits"
    }
    impl_grpc_handler! {
        get_quota_limits,
        pm::GetQuotaLimitsRequest => STREAM(GetQuotaLimitsStream, pm::GetQuotaLimitsResponse),
        "Get quota limits"
    }
    impl_grpc_handler! {
        get_quota_usage,
        pm::GetQuotaUsageRequest => STREAM(GetQuotaUsageStream, pm::GetQuotaUsageResponse),
        "Get quota usage"
    }

    impl_grpc_handler! {
        get_license,
        pm::GetLicenseRequest => pm::GetLicenseResponse,
        "Get license"
    }
}

/// Serve gRPC requests on the `grpc_port` extracted from the config
pub(crate) fn serve(app: RuntimeApp, mut shutdown: RunStateHandle) -> Result<()> {
    let builder = Server::builder();

    // If gRPC TLS is enabled, configure the server accordingly
    let mut builder = if !app.info.user_config.tls_disable {
        let tls_cert = std::fs::read(&app.info.user_config.tls_cert_file).with_context(|| {
            format!(
                "Could not read TLS certificate file {:?}",
                &app.info.user_config.tls_cert_file
            )
        })?;
        let tls_key = std::fs::read(&app.info.user_config.tls_key_file).with_context(|| {
            format!(
                "Could not read TLS key file {:?}",
                &app.info.user_config.tls_key_file
            )
        })?;

        builder
            .tls_config(ServerTlsConfig::new().identity(Identity::from_pem(tls_cert, tls_key)))?
    } else {
        log::warn!("gRPC server running with TLS disabled");
        builder
    };

    let app2 = app.clone();
    let service = pm::management_server::ManagementServer::with_interceptor(
        ManagementService { app: app.clone() },
        move |req: Request<()>| {
            // If authentication is enabled, require the secret passed with every request
            if let Some(required_secret) = app2.info.auth_secret {
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

    let serve_addr = SocketAddr::new(
        if app.info.use_ipv6 {
            Ipv6Addr::UNSPECIFIED.into()
        } else {
            Ipv4Addr::UNSPECIFIED.into()
        },
        app.info.user_config.grpc_port,
    );

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

/// Fails if the management is in pre shutdown state
fn fail_on_pre_shutdown(app: &impl App) -> Result<()> {
    if app.is_pre_shutdown() {
        return Err(anyhow!("Management is shutting down")).status_code(Code::Unavailable);
    }

    Ok(())
}

/// Fails with "Unauthenticated" if the given license feature is not enabled
fn fail_on_missing_license(app: &impl App, feature: LicensedFeature) -> Result<()> {
    app.verify_licensed_feature(feature)
        .status_code(Code::Unauthenticated)
}
