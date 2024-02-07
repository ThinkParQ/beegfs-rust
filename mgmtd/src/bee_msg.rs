//! BeeMsg dispatcher and handlers
//!
//! Dispatches the incoming requests (coming from the BeeMsg connection pool), takes appropriate
//! action in the matching handler and provides a response.

use crate::context::Context;
use crate::db;
use crate::error::TypedError;
use crate::types::{NodeType, NodeTypeServer};
use anyhow::{bail, Result};
use shared::bee_msg::misc::{GenericResponse, TRY_AGAIN};
use shared::bee_msg::{Msg, OpsErr};
use shared::bee_serde::Serializable;
use shared::conn::msg_dispatch::*;
use shared::log_error_chain;
use std::collections::HashMap;

mod buddy_group;
mod misc;
mod node;
mod quota;
mod storage_pool;
mod target;

/// To handle a message, implement this and add it to the dispatch list. If the message
/// should return a response, set `Response` accordingly, otherwise set it to ();
trait Handler {
    type Response;
    async fn handle(self, ctx: &Context, req: &mut impl Request) -> Self::Response;
}

/// Takes a generic request, deserialized the message and dispatches it to the handler in the
/// appropriate submodule.
///
/// Concrete messages to handle must implement the `Handler` trait and added to the macro call
/// within this function like this:
/// ```ignore
/// ...
/// def::SomeMessage => def::SomeRespMessage,
/// ...
/// ```
/// The `=> def::SomeRespMessage` is optional and only required if a response shall be sent after
/// handling. In this case, the `Response` associated type must be set to the respective response
/// message.
///
/// If the function returns nothing (`()`), no response will be sent. Make sure this matches the
/// expecation of the requester.
pub(crate) async fn dispatch_request(ctx: &Context, mut req: impl Request) -> anyhow::Result<()> {
    /// This macro creates the dispatching match statement
    ///
    /// Expects a comma separated list of dispatch directives in the following form:
    /// ```ignore
    /// dispatch_msg!(
    ///     path::to::IncomingMsg => path::to::ResponseMsg,
    ///     path::to::AnotherMsg, // No response
    /// );
    /// ```
    macro_rules! dispatch_msg {
        ($($msg_type:path $(=> $resp_msg_type:path)?),* $(,)?) => {
            // Match on the message ID provided by the request
            match req.msg_id() {
                $(
                    <$msg_type>::ID => {
                        // Messages with and without a response need separate handling
                        dispatch_msg!(@HANDLE $msg_type, $($resp_msg_type)?);
                    }
                ),*

                // Handle unspecified msg IDs
                id => {
                    log::warn!("UNHANDLED INCOMING from {:?} with ID {id}", req.addr());

                    // Signal to the caller that the msg is not handled. The generic response
                    // doesnt have a code for this case, so we just send `TRY_AGAIN` with an
                    // appropriate description.
                    req.respond(&GenericResponse {
                        code: TRY_AGAIN,
                        description: "Unhandled msg".into(),
                    }).await?;

                    Ok(())
                }
            }
        };

        // Handle messages with a response
        (@HANDLE $msg_type:path, $resp_msg_type:path) => {
            // Deserialize into the specified BeeGFS message
            let des: $msg_type = req.deserialize_msg()?;
            log::debug!("INCOMING from {:?}: {:?}", req.addr(), des);

            // Call the specified handler and receive the response
            let response: $resp_msg_type = des.handle(ctx, &mut req).await;

            log::debug!("PROCESSED from {:?}. Responding: {:?}", req.addr(), response);

            // Process the response
            return req.respond(&response).await;
        };

        // Handle messages without a response
        (@HANDLE $msg_type:path,) => {
            // Deserialize into the specified BeeGFS message
            let des: $msg_type = req.deserialize_msg()?;
            log::debug!("INCOMING from {:?}: {:?}", req.addr(), des);

            // No response
            des.handle(ctx, &mut req).await;

            log::debug!("PROCESSED from {:?}", req.addr());

            return Ok(());
        };
    }

    use shared::bee_msg::*;

    // Defines the concrete message to be handled by which handler. See function description for
    // details.
    dispatch_msg!(
        buddy_group::GetMirrorBuddyGroups => buddy_group::GetMirrorBuddyGroupsResp,
        buddy_group::GetStatesAndBuddyGroups => buddy_group::GetStatesAndBuddyGroupsResp,
        buddy_group::RemoveBuddyGroup => buddy_group::RemoveBuddyGroupResp,
        buddy_group::SetMetadataMirroring => buddy_group::SetMetadataMirroringResp,
        buddy_group::SetMirrorBuddyGroup => buddy_group::SetMirrorBuddyGroupResp,
        buddy_group::SetMirrorBuddyGroupResp,
        misc::Ack,
        misc::AuthenticateChannel,
        misc::GetNodeCapacityPools => misc::GetNodeCapacityPoolsResp,
        misc::PeerInfo,
        misc::RefreshCapacityPools => misc::Ack,
        misc::SetChannelDirect,
        node::GetNodes => node::GetNodesResp,
        node::Heartbeat => misc::Ack,
        node::HeartbeatRequest => node::Heartbeat,
        node::RegisterNode => node::RegisterNodeResp,
        node::RemoveNode => node::RemoveNodeResp,
        node::RemoveNodeResp,
        quota::GetDefaultQuota => quota::GetDefaultQuotaResp,
        quota::GetQuotaInfo => quota::GetQuotaInfoResp,
        quota::RequestExceededQuota => quota::RequestExceededQuotaResp,
        quota::SetDefaultQuota => quota::SetDefaultQuotaResp,
        quota::SetQuota => quota::SetQuotaResp,
        storage_pool::AddStoragePool => storage_pool::AddStoragePoolResp,
        storage_pool::GetStoragePools => storage_pool::GetStoragePoolsResp,
        storage_pool::ModifyStoragePool => storage_pool::ModifyStoragePoolResp,
        storage_pool::RemoveStoragePool => storage_pool::RemoveStoragePoolResp,
        target::ChangeTargetConsistencyStates => target::ChangeTargetConsistencyStatesResp,
        target::GetTargetMappings => target::GetTargetMappingsResp,
        target::GetTargetStates => target::GetTargetStatesResp,
        target::MapTargetsResp,
        target::MapTargets => target::MapTargetsResp,
        target::RegisterTarget => target::RegisterTargetResp,
        target::SetStorageTargetInfo => target::SetStorageTargetInfoResp,
        target::SetTargetConsistencyStates => target::SetTargetConsistencyStatesResp,
        target::UnmapTarget => target::UnmapTargetResp,
    )
}

pub async fn notify_nodes<M: Msg + Serializable>(
    ctx: &Context,
    node_types: &'static [NodeType],
    msg: &M,
) {
    log::debug!(target: "mgmtd::msg", "NOTIFICATION to {:?}: {:?}",
            node_types, msg);

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
        log_error_chain!(
            err,
            "Notification msg could not be send to all nodes: {msg:?}"
        );
    }
}
