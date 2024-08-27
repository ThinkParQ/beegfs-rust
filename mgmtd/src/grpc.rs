//! gRPC server and handlers

use crate::bee_msg::notify_nodes;
use crate::context::Context;
use crate::db;
use crate::license::LicensedFeature;
use crate::types::{ResolveEntityId, SqliteEnumExt};
use anyhow::{bail, Context as AContext, Result};
use protobuf::{beegfs as pb, management as pm};
use rusqlite::{params, OptionalExtension, Transaction};
use shared::shutdown::Shutdown;
use shared::types::*;
use shared::{error_chain, log_error_chain};
use sqlite::{check_affected_rows, ConnectionExt, TransactionExt};
use sqlite_check::sql;
use std::fmt::Debug;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use tokio::sync::mpsc;
use tokio_stream::Stream;
use tonic::transport::{Identity, Server, ServerTlsConfig};
use tonic::{Code, Request, Response, Status};

mod buddy_group;
mod license;
mod misc;
mod node;
mod pool;
mod quota;
mod target;

/// Unwraps an optional proto message field . If `None`, errors out providing the fields name in the
/// error message.
///
/// Meant for unwrapping optional protobuf fields that are actually mandatory.
pub(crate) fn required_field<T>(f: Option<T>) -> Result<T> {
    f.ok_or_else(|| ::anyhow::anyhow!("missing required {} field", std::any::type_name::<T>()))
}

fn needs_license(ctx: &Context, feature: LicensedFeature) -> Result<(), Status> {
    ctx.lic
        .verify_feature(feature)
        .map_err(|e| Status::new(Code::Unauthenticated, e.to_string()))
}

/// Serve gRPC requests on the `grpc_port` extracted from the config
pub(crate) fn serve(ctx: Context, mut shutdown: Shutdown) -> Result<()> {
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

/// Wrapper around the stream channel sender
#[derive(Debug, Clone)]
struct StreamSender<Msg>(mpsc::Sender<std::result::Result<Msg, Status>>);

impl<Msg: Send + Sync + 'static> StreamSender<Msg> {
    /// Sends a msg to the stream
    async fn send(&self, value: Msg) -> Result<()> {
        self.0.send(Ok(value)).await?;
        Ok(())
    }
}

/// Convenience alias for the gRPC response stream future
type RespStream<T> = Pin<Box<dyn Stream<Item = std::result::Result<T, Status>> + Send>>;

/// Stream back response messages to the requester if the rpc expects it.
/// Provide the source_fn function/closure to generate the streams results.
///
/// The source_fn function accepts anyhow::Result as return value. If it is an error and its inner
/// type is tonic::Status, it will be sent to the client as given. This can be used to control
/// which gRPC status to send back. For any other error, a generic Code::Internal will be
/// sent back. Finally, if the Result is Ok, the stream will close normally (meaning "success").
///
/// buf_size determines the size of the ready-to-send message buffer. For most callers, especially
/// those who obtain all the needed data at once, this doesn't make a big difference and a small
/// buffer size like 16 should do it - if the response messages are already generated in memory,
/// there is likely no benefit from an extra buffer. Putting a big number might even hurt in this
/// case as the big allocation takes extra effort.
/// However, if data is fetched and streamed page after page, it's a different story. Depending
/// on how long it takes to fetch and generate each page of response messages, a big number can
/// make sense to allow the task to already start fetching the next page while the current one is
/// still being streamed. If there are moments during request handling where the buffer runs out of
/// sendable messages while fetching the next page of data, increasing the buffer increases
/// throughput. On the other hand, if most requests only fetch a fraction of a max page size, a big
/// buffer doesn't really make sense, it just wastes memory. The implementor should take that into
/// account.
fn resp_stream<RespMsg, SourceFn, Fut>(buf_size: usize, source_fn: SourceFn) -> RespStream<RespMsg>
where
    RespMsg: Send + Sync + 'static,
    SourceFn: FnOnce(StreamSender<RespMsg>) -> Fut + Send + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    let (tx, rx) = mpsc::channel(buf_size.max(1));

    tokio::spawn(async move {
        if let Err(err) = source_fn(StreamSender(tx.clone())).await {
            // If this is the result of a closed receive channel (e.g. client cancels
            // receiving early), we don't want to log this as an error.
            if err.is::<mpsc::error::SendError<std::result::Result<RespMsg, Status>>>() {
                log::debug!(
                    "response stream of {} got interrupted: receiver closed the channel",
                    std::any::type_name::<RespMsg>()
                        .split("::")
                        .last()
                        .expect("RespMsg implementor name is never empty")
                );
                return;
            }

            log_error_chain!(
                err,
                "response stream of {}",
                std::any::type_name::<RespMsg>()
                    .split("::")
                    .last()
                    .expect("RespMsg implementor name is never empty")
            );

            match err.downcast::<Status>() {
                // If source_fn returned a gRPC status as error, send it. This allows the
                // function to control the error response
                Ok(status) => {
                    let _ = tx.send(Err(status)).await;
                }
                // If it is any other error, send a generic gRPC error
                Err(err) => {
                    let resp = Err(Status::new(Code::Internal, format!("{err:#}")));
                    let _ = tx.send(resp).await;
                }
            }
        }
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Box::pin(stream)
}

/// Autoimplements a gRPC handle function as expected by impl pm::management_server::Management
/// (which itself is auto-defined by the protobuf definition) for ManagementService, forwards to the
/// actual handler function and handles errors returned from the it. One call of the macro
/// implements one function. Must be called from within the impl block below (see there for usage
/// examples).
///
/// The handler must return anyhow::Result<$resp_msg> or in case of a stream,
/// anyhow::Result<$resp_stream>. If the result is an error and can be downcasted to
/// tonic::Status, it will be sent to the client as given. This can be used to control which gRPC
/// status to send back. For any other error, a generic Code::Internal will be sent back. Finally,
/// if the Result is Ok, a tonic::Response containing will be returned.
macro_rules! impl_handler {
    // Implements the function for a response stream RPC.
    ($impl_fn:ident => $handle_fn:path, $req_msg:path => STREAM($resp_stream:ident, $resp_msg:path), $ctx_str:literal) => {
        // A response stream RPC requires to define a `<MsgName>Stream` associated type and use this
        // as the response for the handler.
        type $resp_stream = RespStream<$resp_msg>;

        impl_handler!(@INNER $impl_fn => $handle_fn, $req_msg => Self::$resp_stream, $ctx_str);
    };

    // Implements the function for a unary RPC.
    ($impl_fn:ident => $handle_fn:path, $req_msg:path => $resp_msg:path, $ctx_str:literal) => {
        impl_handler!(@INNER $impl_fn => $handle_fn, $req_msg => $resp_msg, $ctx_str);
    };

    // Generates the actual function. Note that we implement the `async fn` manually to avoid having
    // to use `#[tonic::async_trait]`. This is exactly how that macro does it in the background, but
    // we can't rely on that here within this macro as attribute macros are evaluated first.
    (@INNER $impl_fn:ident => $handle_fn:path, $req_msg:path => $resp_msg:path, $ctx_str:literal) => {
        fn $impl_fn<'a, 'async_trait>(
            &'a self,
            req: Request<$req_msg>,
        ) -> Pin<Box<dyn Future<Output = Result<Response<$resp_msg>, Status>> + Send + 'async_trait>>
        where
            'a: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                let res = $handle_fn(self.ctx.clone(), req.into_inner()).await;

                match res {
                    Ok(res) => Ok(Response::new(res)),
                    Err(err) => {
                        log_error_chain!(err, concat!($ctx_str, " failed"));

                        match err.downcast::<Status>() {
                            // If handle_fn returned a gRPC status as error, send it. This allows the
                            // function to control the error response
                            Ok(status) => Err(status),
                            // If it is any other error, send a generic gRPC error
                            Err(err) => Err(Status::new(Code::Internal, format!("{err:#}"))),
                        }
                    }
                }
            })
        }
    };
}

/// Management gRPC service implementation struct
#[derive(Debug)]
pub(crate) struct ManagementService {
    pub ctx: Context,
}

/// Implementation of the management gRPC service. Use the impl_handler! macro to implement each
/// function.
///
/// However, if a function should be implemented manually using an async fn, re-add the
/// #[tonic::async_trait] macro (or it will not work).
impl pm::management_server::Management for ManagementService {
    // Example: Implement pm::management_server::Management::set_alias using the impl_handler macro
    impl_handler! {
        // <the function to implement (as defined by the trait)> => <the actual, custom handler function to call>,
        set_alias => misc::set_alias,
        // <request message passed to the fn impl (as defined by the trait)> => <response message,
        // returned by the fn impl (as defined by the trait)>,
        pm::SetAliasRequest => pm::SetAliasResponse,
        // <context string for logged errors>
        "Set alias"
    }

    impl_handler! {
        get_nodes => node::get,
        pm::GetNodesRequest => pm::GetNodesResponse,
        "Get nodes"
    }
    impl_handler! {
        delete_node => node::delete,
        pm::DeleteNodeRequest => pm::DeleteNodeResponse,
        "Delete node"
    }

    impl_handler! {
        get_targets => target::get,
        pm::GetTargetsRequest => pm::GetTargetsResponse,
        "Get targets"
    }
    impl_handler! {
        delete_target => target::delete,
        pm::DeleteTargetRequest => pm::DeleteTargetResponse,
        "Delete target"
    }
    impl_handler! {
        set_target_state => target::set_state,
        pm::SetTargetStateRequest => pm::SetTargetStateResponse,
        "Set target state"
    }

    impl_handler! {
        get_pools => pool::get,
        pm::GetPoolsRequest => pm::GetPoolsResponse,
        "Get pools"
    }
    impl_handler! {
        create_pool => pool::create,
        pm::CreatePoolRequest => pm::CreatePoolResponse,
        "Create pool"
    }
    impl_handler! {
        assign_pool => pool::assign,
        pm::AssignPoolRequest => pm::AssignPoolResponse,
        "Assign pool"
    }
    impl_handler! {
        delete_pool => pool::delete,
        pm::DeletePoolRequest => pm::DeletePoolResponse,
        "Delete pool"
    }

    impl_handler! {
        get_buddy_groups => buddy_group::get,
        pm::GetBuddyGroupsRequest => pm::GetBuddyGroupsResponse,
        "Get buddy groups"
    }
    impl_handler! {
        create_buddy_group => buddy_group::create,
        pm::CreateBuddyGroupRequest => pm::CreateBuddyGroupResponse,
        "Create buddy group"
    }
    impl_handler! {
        delete_buddy_group => buddy_group::delete,
        pm::DeleteBuddyGroupRequest => pm::DeleteBuddyGroupResponse,
        "Delete buddy group"
    }
    impl_handler! {
        mirror_root_inode => buddy_group::mirror_root_inode,
        pm::MirrorRootInodeRequest => pm::MirrorRootInodeResponse,
        "Mirror root inode"
    }

    impl_handler! {
        set_default_quota_limits => quota::set_default_quota_limits,
        pm::SetDefaultQuotaLimitsRequest => pm::SetDefaultQuotaLimitsResponse,
        "Set default quota limits"
    }
    impl_handler! {
        set_quota_limits => quota::set_quota_limits,
        pm::SetQuotaLimitsRequest => pm::SetQuotaLimitsResponse,
        "Set quota limits"
    }
    impl_handler! {
        get_quota_limits => quota::get_quota_limits,
        pm::GetQuotaLimitsRequest => STREAM(GetQuotaLimitsStream, pm::GetQuotaLimitsResponse),
        "Get quota limits"
    }
    impl_handler! {
        get_quota_usage => quota::get_quota_usage,
        pm::GetQuotaUsageRequest => STREAM(GetQuotaUsageStream, pm::GetQuotaUsageResponse),
        "Get quota usage"
    }

    impl_handler! {
        get_license => license::get,
        pm::GetLicenseRequest => pm::GetLicenseResponse,
        "Get license"
    }
}
