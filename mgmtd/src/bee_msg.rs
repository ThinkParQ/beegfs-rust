//! BeeMsg dispatcher and handlers
//!
//! Dispatches the incoming requests (coming from the BeeMsg connection pool), takes appropriate
//! action in the matching handler and provides a response.

use crate::context::Context;
use crate::db;
use crate::error::TypedError;
use crate::types::*;
use anyhow::{anyhow, bail, Context as AContext, Result};
use shared::bee_msg::misc::{GenericResponse, TRY_AGAIN};
use shared::bee_msg::{Msg, OpsErr};
use shared::bee_serde::{Deserializable, Serializable};
use shared::conn::msg_dispatch::*;
use shared::types::*;
use sqlite::{ConnectionExt, TransactionExt};
use sqlite_check::sql;
use std::collections::HashMap;
use std::fmt::Display;

mod buddy_group;
mod misc;
mod node;
mod quota;
mod storage_pool;
mod target;

/// Msg request handler for requests where no response is expected.
/// To handle a message, implement this and add it to the dispatch list with `=> _`.
trait HandleNoResponse: Msg + Deserializable {
    async fn handle(self, ctx: &Context, req: &mut impl Request) -> Result<()>;
}

/// Msg request handler for requests where a response is expected.
/// To handle a message, implement this and add it to the dispatch list with `=> R`.
trait HandleWithResponse: Msg + Deserializable {
    type Response: Msg + Serializable;
    async fn handle(self, ctx: &Context, req: &mut impl Request) -> Result<Self::Response>;

    /// Defines the message to send back on an error during `handle()`. Defaults to
    /// `Response::default()`.
    fn error_response() -> Self::Response {
        Self::Response::default()
    }
}

#[derive(Debug)]
struct PreShutdownError();

impl Display for PreShutdownError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Management is shutting down")
    }
}

/// Takes a generic request, deserialized the message and dispatches it to the handler in the
/// appropriate submodule.
///
/// Concrete messages to handle must implement the `Handler` trait and added to the macro call
/// within this function like this:
/// ```ignore
/// ...
/// {path::to::Msg => R, "Message context"}
/// ...
/// ```
/// The `=> R` tells the macro that this message handler returns a response. For non-response
/// messages, put a `=> _` there. Implement the appropriate handler trait for that message or you
/// will get errors
pub(crate) async fn dispatch_request(ctx: &Context, mut req: impl Request) -> Result<()> {
    /// Creates the dispatching match statement
    macro_rules! dispatch_msg {
        ($({$msg_type:path => $r:tt, $ctx_str:literal})*) => {
            // Match on the message ID provided by the request
            match req.msg_id() {
                $(
                    <$msg_type>::ID => {
                        let des: $msg_type = req.deserialize_msg().with_context(|| {
                            format!(
                                "{} ({}) from {:?}",
                                stringify!($msg_type),
                                req.msg_id(),
                                req.addr()
                            )
                        })?;

                        log::trace!("INCOMING from {:?}: {:?}", req.addr(), des);

                        let res = des.handle(ctx, &mut req).await;
                        dispatch_msg!(@HANDLE res, $msg_type => $r, $ctx_str)
                    }
                ),*

                _ => handle_unspecified_msg(req).await
            }
        };

        // Handler result with a response
        (@HANDLE $res:ident, $msg_type:path => R, $ctx_str:literal) => {{
            let resp = match $res {
                Ok(resp) => {
                    log::trace!("PROCESSED from {:?}. Responding: {:?}", req.addr(), resp);
                    resp
                }
                Err(err) => {
                    if let Some(pse) = err.downcast_ref::<PreShutdownError>() {
                    log::debug!("{}: {pse}", $ctx_str);
                        return req.respond(&GenericResponse {
                            code: TRY_AGAIN,
                            description: pse.to_string().into_bytes(),
                        }).await;
                    }

                    log::error!("{}: {err:#}", $ctx_str);
                    <$msg_type>::error_response()
                }
            };

            req.respond(&resp).await
        }};

        // Handler result without a response
        (@HANDLE $res:ident, $msg_type:path => _, $ctx_str:literal) => {{
            $res.unwrap_or_else(|err| {
                log::error!("{}: {err:#}", $ctx_str);
            });

            log::trace!("PROCESSED from {:?}", req.addr());
            Ok(())
        }};
    }

    use shared::bee_msg::*;

    // Creates the match block for message dispatching
    dispatch_msg! {
        {buddy_group::GetMirrorBuddyGroups => R, "Get buddy groups"}
        {buddy_group::GetStatesAndBuddyGroups => R, "Get states and buddy groups"}
        {buddy_group::SetMirrorBuddyGroupResp => _, "SetMirrorBuddyGroupResp"}
        {misc::Ack => _, "Ack"}
        {misc::AuthenticateChannel => _, "Authenticate connection"}
        {misc::GetNodeCapacityPools => R, "Get capacity pools"}
        {misc::PeerInfo => _, "PeerInfo"}
        {misc::RefreshCapacityPools => R, "Refresh capacity pools"}
        {misc::SetChannelDirect => _, "SetChannelDirect"}
        {node::GetNodes => R, "Get nodes"}
        {node::Heartbeat => R, "Heartbeat"}
        {node::HeartbeatRequest => R, "Request heartbeat"}
        {node::RegisterNode => R, "Register node"}
        {node::RemoveNode => R, "Remove node"}
        {node::RemoveNodeResp => _, "RemoveNodeResp"}
        {quota::RequestExceededQuota => R, "Request exceeded quota"}
        {storage_pool::GetStoragePools => R, "Get storage pools"}
        {target::ChangeTargetConsistencyStates => R, "Change target consistency states"}
        {target::GetTargetMappings => R, "Get target mappings"}
        {target::GetTargetStates => R, "Get target states"}
        {target::MapTargetsResp => _, "MapTargetsResp"}
        {target::MapTargets => R, "Map targets"}
        {target::RegisterTarget => R, "Register target"}
        {target::SetStorageTargetInfo => R, "Set storage target info"}
        {target::SetTargetConsistencyStates => R, "Set target consistency states"}
    }
}

async fn handle_unspecified_msg(req: impl Request) -> Result<()> {
    log::warn!(
        "Unhandled msg INCOMING from {:?} with ID {}",
        req.addr(),
        req.msg_id()
    );

    // Signal to the caller that the msg is not handled. The generic response
    // doesnt have a code for this case, so we just send `TRY_AGAIN` with an
    // appropriate description.
    req.respond(&GenericResponse {
        code: TRY_AGAIN,
        description: "Unhandled msg".into(),
    })
    .await?;

    Ok(())
}

/// Checks if the management is in pre shutdown state
fn fail_on_pre_shutdown(ctx: &Context) -> Result<()> {
    if ctx.run_state.pre_shutdown() {
        return Err(anyhow!(PreShutdownError {}));
    }

    Ok(())
}

pub async fn notify_nodes<M: Msg + Serializable>(
    ctx: &Context,
    node_types: &'static [NodeType],
    msg: &M,
) {
    log::trace!("NOTIFICATION to {:?}: {:?}", node_types, msg);

    if let Err(err) = async {
        for t in node_types {
            let nodes = ctx.db.op(move |tx| db::node::get_with_type(tx, *t)).await?;

            ctx.conn
                .broadcast_datagram(nodes.into_iter().map(|e| e.uid), msg)
                .await?;
        }

        Ok(()) as Result<_>
    }
    .await
    {
        log::error!("Notification could not be send to all nodes: {err:#}");
    }
}
