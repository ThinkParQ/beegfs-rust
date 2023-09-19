//! Message dispatcher and handle functions
//!
//! This is the heart of managment as is dispatches the incoming requests (coming from the
//! connection pool), takes appropriate action in the matching handler and provides a response.

use crate::context::Context;
use crate::db;
use crate::db::TypedError;
use anyhow::{bail, Result};
use shared::conn::msg_dispatch::*;
use shared::msg::{GenericResponse, Msg};
use shared::types::{NodeType, OpsErr, TRY_AGAIN};
use shared::{log_error_chain, msg};
use std::collections::HashMap;

mod ack;
mod add_storage_pool;
mod authenticate_channel;
mod change_target_consistency_states;
mod get_default_quota;
mod get_mirror_buddy_groups;
mod get_node_capacity_pools;
mod get_nodes;
mod get_quota_info;
mod get_states_and_buddy_groups;
mod get_storage_pools;
mod get_target_mappings;
mod get_target_states;
mod heartbeat;
mod heartbeat_request;
mod map_targets;
mod map_targets_resp;
mod modify_storage_pool;
mod peer_info;
mod refresh_capacity_pools;
mod register_node;
mod register_target;
mod remove_buddy_group;
mod remove_node;
mod remove_node_resp;
mod remove_storage_pool;
mod request_exceeded_quota;
mod set_channel_direct;
mod set_default_quota;
mod set_metadata_mirroring;
mod set_mirror_buddy_group;
mod set_mirror_buddy_group_resp;
mod set_quota;
mod set_storage_target_info;
mod set_target_consistency_states;
mod unmap_target;

/// Takes a generic request, deserialized the message and dispatches it to the handler in the
/// appropriate submodule.
///
/// Concrete messages to handle must be added into the macro call within this function like this:
/// ```ignore
/// ...
/// msg::SomeMessage => some_message_handle_module,
/// ...
/// ```
///
/// A handler submodule must be created. The submodule must contain a function with the following
/// signature:
///
/// ```ignore
/// pub(super) async fn handle(
///     msg: msg::SomeMessage,
///     ctx: &impl AppContext,
///     req: &impl Request,
/// ) -> msg::SomeMessageResponse {
///     // Handling code
/// }
/// ```
///
/// If the function returns nothing (`()`), no response will be sent. Make sure this matches the
/// expecation of the requester.
pub(crate) async fn dispatch_request(ctx: &Context, mut req: impl Request) -> anyhow::Result<()> {
    /// This macro creates the dispatching match statement
    ///
    /// Expects a comma separated list of dispatch directives in the following form:
    /// ```ignore
    /// dispatch_msg!(
    ///     path::to::IncomingMsg => handle_module => path::to::ResponseMsg,
    ///     path::to::AnotherMsg => handle_module, // No response
    /// );
    /// ```
    macro_rules! dispatch_msg {
        ($($msg_type:path => $handle_mod:ident $(=> $resp_msg_type:path)? ),* $(,)?) => {
            // Match on the message ID provided by the request
            match req.msg_id() {
                $(
                    <$msg_type>::ID => {
                        // Messages with and without a response need separate handling
                        dispatch_msg!(@HANDLE $msg_type, $handle_mod, $($resp_msg_type)?);
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
        (@HANDLE $msg_type:path, $handle_mod:ident, $resp_msg_type:path) => {
            // Deserialize into the specified BeeGFS message
            let des: $msg_type = req.deserialize_msg()?;
            log::debug!("INCOMING from {:?}: {:?}", req.addr(), des);


            // Call the specified handler and receive the response
            let response: $resp_msg_type = $handle_mod::handle(des, ctx, &mut req).await;

            log::debug!("PROCESSED from {:?}. Responding: {:?}", req.addr(), response);

            // Process the response
            return req.respond(&response).await;
        };

        // Handle messages without a response
        (@HANDLE $msg_type:path, $handle_mod:ident, ) => {
            // Deserialize into the specified BeeGFS message
            let des: $msg_type = req.deserialize_msg()?;
            log::debug!("INCOMING from {:?}: {:?}", req.addr(), des);

            // No response
            $handle_mod::handle(des, ctx, &mut req).await;

            log::debug!("PROCESSED from {:?}", req.addr());

            return Ok(());
        };
    }

    // Defines the concrete message to be handled by which handler. See function description for
    // details.
    dispatch_msg!(
        // TCP
        msg::RegisterNode => register_node => msg::RegisterNodeResp,
        msg::RemoveNode => remove_node  => msg::RemoveNodeResp,
        msg::GetNodes => get_nodes => msg::GetNodesResp ,
        msg::RegisterTarget => register_target => msg::RegisterTargetResp ,
        msg::MapTargets => map_targets => msg::MapTargetsResp ,
        msg::GetTargetMappings => get_target_mappings => msg::GetTargetMappingsResp ,
        msg::GetTargetStates => get_target_states => msg::GetTargetStatesResp ,
        msg::GetStoragePools => get_storage_pools => msg::GetStoragePoolsResp ,
        msg::GetStatesAndBuddyGroups => get_states_and_buddy_groups => msg::GetStatesAndBuddyGroupsResp ,
        msg::GetNodeCapacityPools => get_node_capacity_pools => msg::GetNodeCapacityPoolsResp ,
        msg::ChangeTargetConsistencyStates => change_target_consistency_states => msg::ChangeTargetConsistencyStatesResp ,
        msg::SetStorageTargetInfo => set_storage_target_info => msg::SetStorageTargetInfoResp ,
        msg::RequestExceededQuota => request_exceeded_quota => msg::RequestExceededQuotaResp ,
        msg::GetMirrorBuddyGroups => get_mirror_buddy_groups => msg::GetMirrorBuddyGroupsResp ,
        msg::SetChannelDirect => set_channel_direct,
        msg::PeerInfo => peer_info,
        msg::AddStoragePool => add_storage_pool => msg::AddStoragePoolResp ,
        msg::ModifyStoragePool => modify_storage_pool => msg::ModifyStoragePoolResp ,
        msg::RemoveStoragePool => remove_storage_pool => msg::RemoveStoragePoolResp ,
        msg::UnmapTarget => unmap_target => msg::UnmapTargetResp ,
        msg::SetDefaultQuota => set_default_quota => msg::SetDefaultQuotaResp ,
        msg::GetDefaultQuota => get_default_quota => msg::GetDefaultQuotaResp ,
        msg::SetQuota => set_quota => msg::SetQuotaResp ,
        msg::GetQuotaInfo => get_quota_info => msg::GetQuotaInfoResp ,
        msg::AuthenticateChannel => authenticate_channel,
        msg::SetMirrorBuddyGroup => set_mirror_buddy_group => msg::SetMirrorBuddyGroupResp ,
        msg::RemoveBuddyGroup => remove_buddy_group => msg::RemoveBuddyGroupResp ,
        msg::SetMetadataMirroring => set_metadata_mirroring => msg::SetMetadataMirroringResp ,
        msg::SetTargetConsistencyStates => set_target_consistency_states => msg::SetTargetConsistencyStatesResp ,

        // UDP
        msg::Heartbeat => heartbeat => msg::Ack ,
        msg::HeartbeatRequest => heartbeat_request => msg::Heartbeat,
        msg::RefreshCapacityPools => refresh_capacity_pools => msg::Ack ,
        msg::Ack => ack,
        msg::RemoveNodeResp => remove_node_resp,
        msg::MapTargetsResp => map_targets_resp,
        msg::SetMirrorBuddyGroupResp => set_mirror_buddy_group_resp,
    )
}

pub async fn notify_nodes(ctx: &Context, node_types: &'static [NodeType], msg: &impl Msg) {
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
