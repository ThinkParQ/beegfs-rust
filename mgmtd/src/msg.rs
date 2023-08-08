//! Message dispatcher and handle functions
//!
//! This is the heart of managment as is dispatches the incoming requests (coming from the
//! connection pool), takes appropriate action in the matching handler and provides a response.

use crate::app_context::AppContext;
use crate::db;
use crate::db::{DbError, DbResult};
use anyhow::{bail, Result};
use shared::conn::msg_dispatch::*;
use shared::msg::{self, GenericResponse, Msg};
use shared::*;
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
pub(crate) async fn dispatch_request(
    ctx: &impl AppContext,
    mut req: impl Request,
) -> anyhow::Result<()> {
    // This macro creates the dispatching match statement
    macro_rules! dispatch_msg {
        ($($msg_type:path => $handle_mod:ident),* $(,)?) => {
            // Match on the message ID provided by the request
            match req.msg_id() {
                $(
                    <$msg_type>::ID => {
                        // Deserialize into the specified BeeGFS message
                        let des: $msg_type = req.deserialize_msg()?;
                        log::debug!("INCOMING from {:?}: {:?}", req.addr(), des);

                        #[allow(clippy::unnecessary_mut_passed)]
                        // Call the specified handler and receive the response
                        // The response can be `()`, in which case no response will be sent
                        let response = $handle_mod::handle(des, ctx, &mut req).await;

                        log::debug!("PROCESSED from {:?}. Responding: {:?}", req.addr(), response);

                        // Process the response (or non-response) using the ResponseMsg helper
                        ResponseMsg::respond(req, &response).await
                    }
                ),*

                // Handle unspecified msg IDs
                id => {
                    log::warn!("UNHANDLED INCOMING from {:?} with ID {id}", req.addr());

                    // Signal to the caller that the msg is not handled. The generic response
                    // doesnt have a code for this case, so we just send `TRY_AGAIN` with an
                    // appropriate description.
                    ResponseMsg::respond(req, &GenericResponse {
                        code: GenericResponseCode::TRY_AGAIN,
                        description: "Unhandled msg".into(),
                    }).await?;

                    Ok(())
                }
            }
        }
    }

    // Defines the concrete message to be handled by which handler. See function description for
    // details.
    dispatch_msg!(
        // TCP
        msg::RegisterNode => register_node,
        msg::RemoveNode => remove_node,
        msg::GetNodes => get_nodes,
        msg::RegisterTarget => register_target,
        msg::MapTargets => map_targets,
        msg::GetTargetMappings => get_target_mappings,
        msg::GetTargetStates => get_target_states,
        msg::GetStoragePools => get_storage_pools,
        msg::GetStatesAndBuddyGroups => get_states_and_buddy_groups,
        msg::GetNodeCapacityPools => get_node_capacity_pools,
        msg::ChangeTargetConsistencyStates => change_target_consistency_states,
        msg::SetStorageTargetInfo => set_storage_target_info,
        msg::RequestExceededQuota => request_exceeded_quota,
        msg::GetMirrorBuddyGroups => get_mirror_buddy_groups,
        msg::SetChannelDirect => set_channel_direct,
        msg::PeerInfo => peer_info,
        msg::AddStoragePool => add_storage_pool,
        msg::ModifyStoragePool => modify_storage_pool,
        msg::RemoveStoragePool => remove_storage_pool,
        msg::UnmapTarget => unmap_target,
        msg::SetDefaultQuota => set_default_quota,
        msg::GetDefaultQuota => get_default_quota,
        msg::SetQuota => set_quota,
        msg::GetQuotaInfo => get_quota_info,
        msg::AuthenticateChannel => authenticate_channel,
        // msg::SetConfig => set_config,
        // msg::GetConfig => get_config,
        msg::SetMirrorBuddyGroup => set_mirror_buddy_group,
        msg::RemoveBuddyGroup => remove_buddy_group,
        msg::SetMetadataMirroring => set_metadata_mirroring,
        msg::SetTargetConsistencyStates => set_target_consistency_states,

        // UDP
        msg::Heartbeat => heartbeat,
        msg::HeartbeatRequest => heartbeat_request,
        msg::RefreshCapacityPools => refresh_capacity_pools,
        msg::Ack => ack,
        msg::RemoveNodeResp => remove_node_resp,
        msg::MapTargetsResp => map_targets_resp,
        msg::SetMirrorBuddyGroupResp => set_mirror_buddy_group_resp,
    )
}
