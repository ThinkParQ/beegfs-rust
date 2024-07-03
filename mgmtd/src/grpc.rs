//! gRPC server and handlers

use crate::bee_msg::notify_nodes;
use crate::context::Context;
use crate::db;
use crate::license::LicensedFeature;
use crate::types::{ResolveEntityId, SqliteEnumExt};
use anyhow::{bail, Context as AContext, Result};
use protobuf::{beegfs as pb, management as pm};
use rusqlite::{params, OptionalExtension, Transaction};
use shared::error_chain;
use shared::shutdown::Shutdown;
use shared::types::*;
use sqlite::{check_affected_rows, ConnectionExt, TransactionExt};
use sqlite_check::sql;
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

/// Serve gRPC requests on the `grpc_port` extracted from the config
pub(crate) fn serve(ctx: Context, mut shutdown: Shutdown) -> Result<()> {
    let builder = Server::builder();

    // If gRPC TLS is enabled, configure the server accordingly
    let mut builder = if ctx.info.user_config.tls_enable {
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
    RespMsg: Send + 'static,
    SourceFn: FnOnce(StreamSender<RespMsg>) -> Fut + Send + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    let (tx, rx) = mpsc::channel(buf_size.max(1));

    tokio::spawn(async move {
        if let Err(err) = source_fn(StreamSender(tx.clone())).await {
            match err.downcast::<Status>() {
                // If source_fn returned a gRPC status as error, send it. This allows the
                // function to control the error response
                Ok(status) => {
                    log::error!("{status:#}");
                    let _ = tx.send(Err(status)).await;
                }
                // If it is any other error, send a generic gRPC error
                Err(err) => {
                    let resp = Err(Status::new(Code::Internal, format!("{err:#}")));
                    let _ = tx.send(resp).await;
                    log::error!("{err:#}");
                }
            }
        }
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Box::pin(stream)
}

/// gRPC server implementation
#[derive(Debug)]
pub(crate) struct ManagementService {
    pub ctx: Context,
}

fn needs_license(ctx: &Context, feature: LicensedFeature) -> Result<(), Status> {
    ctx.lic
        .verify_feature(feature)
        .map_err(|e| Status::new(Code::Unauthenticated, e.to_string()))
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

    async fn set_target_state(
        &self,
        req: Request<pm::SetTargetStateRequest>,
    ) -> Result<Response<pm::SetTargetStateResponse>, Status> {
        let res = target::set_state(&self.ctx, req.into_inner()).await;

        match res {
            Ok(res) => Ok(Response::new(res)),
            Err(err) => {
                let msg = error_chain!(err, "Set target state failed");
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
        needs_license(&self.ctx, LicensedFeature::Storagepool)?;

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
        needs_license(&self.ctx, LicensedFeature::Storagepool)?;

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
        needs_license(&self.ctx, LicensedFeature::Storagepool)?;

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
        needs_license(&self.ctx, LicensedFeature::Mirroring)?;

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
        needs_license(&self.ctx, LicensedFeature::Mirroring)?;

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

    async fn set_default_quota_limits(
        &self,
        req: Request<pm::SetDefaultQuotaLimitsRequest>,
    ) -> Result<Response<pm::SetDefaultQuotaLimitsResponse>, Status> {
        needs_license(&self.ctx, LicensedFeature::Quota)?;

        let res = quota::set_default_quota_limits(&self.ctx, req.into_inner()).await;

        match res {
            Ok(res) => Ok(Response::new(res)),
            Err(err) => {
                let msg = error_chain!(err, "Setting default quota limits failed");
                log::error!("{msg}");
                Err(Status::new(Code::Internal, msg))
            }
        }
    }

    type GetQuotaLimitsStream = RespStream<pm::GetQuotaLimitsResponse>;

    async fn get_quota_limits(
        &self,
        req: Request<pm::GetQuotaLimitsRequest>,
    ) -> Result<Response<Self::GetQuotaLimitsStream>, Status> {
        needs_license(&self.ctx, LicensedFeature::Quota)?;

        let res = quota::get_quota_limits(self.ctx.clone(), req.into_inner()).await;

        match res {
            Ok(res) => Ok(Response::new(res)),
            Err(err) => {
                let msg = error_chain!(err, "Getting quota limits failed");
                log::error!("{msg}");
                Err(Status::new(Code::Internal, msg))
            }
        }
    }

    async fn set_quota_limits(
        &self,
        req: Request<pm::SetQuotaLimitsRequest>,
    ) -> Result<Response<pm::SetQuotaLimitsResponse>, Status> {
        needs_license(&self.ctx, LicensedFeature::Quota)?;

        let res = quota::set_quota_limits(&self.ctx, req.into_inner()).await;

        match res {
            Ok(res) => Ok(Response::new(res)),
            Err(err) => {
                let msg = error_chain!(err, "Setting quota limits failed");
                log::error!("{msg}");
                Err(Status::new(Code::Internal, msg))
            }
        }
    }

    type GetQuotaUsageStream = RespStream<pm::GetQuotaUsageResponse>;

    async fn get_quota_usage(
        &self,
        req: Request<pm::GetQuotaUsageRequest>,
    ) -> Result<Response<Self::GetQuotaUsageStream>, Status> {
        needs_license(&self.ctx, LicensedFeature::Quota)?;

        let res_stream = quota::get_quota_usage(self.ctx.clone(), req.into_inner()).await;

        match res_stream {
            Ok(res) => Ok(Response::new(res)),
            Err(err) => {
                let msg = error_chain!(err, "Getting quota usage failed");
                log::error!("{msg}");
                Err(Status::new(Code::Internal, msg))
            }
        }
    }

    async fn mirror_root_inode(
        &self,
        req: Request<pm::MirrorRootInodeRequest>,
    ) -> Result<Response<pm::MirrorRootInodeResponse>, Status> {
        let res = buddy_group::mirror_root_inode(&self.ctx, req.into_inner()).await;

        match res {
            Ok(res) => Ok(Response::new(res)),
            Err(err) => {
                let msg = error_chain!(err, "Mirroring root inode failed");
                log::error!("{msg}");
                Err(Status::new(Code::Internal, msg))
            }
        }
    }

    async fn get_license(
        &self,
        req: Request<pm::GetLicenseRequest>,
    ) -> Result<Response<pm::GetLicenseResponse>, Status> {
        let res = license::get(&self.ctx, req.into_inner()).await;

        match res {
            Ok(res) => Ok(Response::new(res)),
            Err(err) => {
                let msg = error_chain!(err, "Getting license failed");
                log::error!("{msg}");
                Err(Status::new(Code::Internal, msg))
            }
        }
    }
}
