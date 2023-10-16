//! Message dispatcher and handle functions
//!
//! This is the heart of managment as is dispatches the incoming requests (coming from the
//! connection pool), takes appropriate action in the matching handler and provides a response.

use crate::context::Context;
use crate::db;
use crate::db::TypedError;
use anyhow::{bail, Result};
use shared::conn::msg_dispatch::*;
use shared::log_error_chain;
use shared::msg::generic_response::{GenericResponse, TRY_AGAIN};
use shared::msg::{Msg, OpsErr};
use shared::types::NodeType;
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
/// def::SomeMessage => some_message_handle_module,
/// ...
/// ```
///
/// A handler submodule must be created. The submodule must contain a function with the following
/// signature:
///
/// ```ignore
/// pub(super) async fn handle(
///     msg: def::SomeMessage,
///     ctx: &impl AppContext,
///     req: &impl Request,
/// ) -> def::SomeMessageResponse {
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

    use shared::msg as def;

    // Defines the concrete message to be handled by which handler. See function description for
    // details.
    dispatch_msg!(
        // TCP
        def::register_node::RegisterNode => register_node => def::register_node::RegisterNodeResp,
        def::remove_node::RemoveNode => remove_node  => def::remove_node::RemoveNodeResp,
        def::get_nodes::GetNodes => get_nodes => def::get_nodes::GetNodesResp ,
        def::register_target::RegisterTarget => register_target => def::register_target::RegisterTargetResp ,
        def::map_targets::MapTargets => map_targets => def::map_targets::MapTargetsResp ,
        def::get_target_mappings::GetTargetMappings => get_target_mappings => def::get_target_mappings::GetTargetMappingsResp ,
        def::get_target_states::GetTargetStates => get_target_states => def::get_target_states::GetTargetStatesResp ,
        def::get_storage_pools::GetStoragePools => get_storage_pools => def::get_storage_pools::GetStoragePoolsResp ,
        def::get_states_and_buddy_groups::GetStatesAndBuddyGroups => get_states_and_buddy_groups => def::get_states_and_buddy_groups::GetStatesAndBuddyGroupsResp ,
        def::get_node_capacity_pools::GetNodeCapacityPools => get_node_capacity_pools => def::get_node_capacity_pools::GetNodeCapacityPoolsResp ,
        def::change_target_consistency_states::ChangeTargetConsistencyStates => change_target_consistency_states => def::change_target_consistency_states::ChangeTargetConsistencyStatesResp ,
        def::set_storage_target_info::SetStorageTargetInfo => set_storage_target_info => def::set_storage_target_info::SetStorageTargetInfoResp ,
        def::request_exceeded_quota::RequestExceededQuota => request_exceeded_quota => def::request_exceeded_quota::RequestExceededQuotaResp ,
        def::get_mirror_buddy_groups::GetMirrorBuddyGroups => get_mirror_buddy_groups => def::get_mirror_buddy_groups::GetMirrorBuddyGroupsResp ,
        def::set_channel_direct::SetChannelDirect => set_channel_direct,
        def::peer_info::PeerInfo => peer_info,
        def::add_storage_pool::AddStoragePool => add_storage_pool => def::add_storage_pool::AddStoragePoolResp ,
        def::modify_storage_pool::ModifyStoragePool => modify_storage_pool => def::modify_storage_pool::ModifyStoragePoolResp ,
        def::remove_storage_pool::RemoveStoragePool => remove_storage_pool => def::remove_storage_pool::RemoveStoragePoolResp ,
        def::unmap_target::UnmapTarget => unmap_target => def::unmap_target::UnmapTargetResp ,
        def::set_default_quota::SetDefaultQuota => set_default_quota => def::set_default_quota::SetDefaultQuotaResp ,
        def::get_default_quota::GetDefaultQuota => get_default_quota => def::get_default_quota::GetDefaultQuotaResp ,
        def::set_quota::SetQuota => set_quota => def::set_quota::SetQuotaResp ,
        def::get_quota_info::GetQuotaInfo => get_quota_info => def::get_quota_info::GetQuotaInfoResp ,
        def::authenticate_channel::AuthenticateChannel => authenticate_channel,
        def::set_mirror_buddy_group::SetMirrorBuddyGroup => set_mirror_buddy_group => def::set_mirror_buddy_group::SetMirrorBuddyGroupResp ,
        def::remove_buddy_group::RemoveBuddyGroup => remove_buddy_group => def::remove_buddy_group::RemoveBuddyGroupResp ,
        def::set_metadata_mirroring::SetMetadataMirroring => set_metadata_mirroring => def::set_metadata_mirroring::SetMetadataMirroringResp ,
        def::set_target_consistency_states::SetTargetConsistencyStates => set_target_consistency_states => def::set_target_consistency_states::SetTargetConsistencyStatesResp ,

        // UDP
        def::heartbeat::Heartbeat => heartbeat => def::ack::Ack ,
        def::heartbeat::HeartbeatRequest => heartbeat_request => def::heartbeat::Heartbeat,
        def::refresh_capacity_pools::RefreshCapacityPools => refresh_capacity_pools => def::ack::Ack ,
        def::ack::Ack => ack,
        def::remove_node::RemoveNodeResp => remove_node_resp,
        def::map_targets::MapTargetsResp => map_targets_resp,
        def::set_mirror_buddy_group::SetMirrorBuddyGroupResp => set_mirror_buddy_group_resp,
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
