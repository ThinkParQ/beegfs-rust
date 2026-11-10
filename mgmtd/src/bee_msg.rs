//! BeeMsg dispatcher and handlers
//!
//! Dispatches the incoming requests (coming from the BeeMsg connection pool), takes appropriate
//! action in the matching handler and provides a response.

use crate::app::*;
use crate::db;
use crate::error::TypedError;
use crate::types::*;
use anyhow::{Context as AContext, Result, anyhow, bail};
use shared::bee_msg::misc::{GenericResponse, TRY_AGAIN};
use shared::bee_msg::{Msg, OpsErr};
use shared::bee_serde::{Deserializable, Serializable};
use shared::conn::msg_dispatch::*;
use shared::types::*;
use sqlite::TransactionExt;
use sqlite_check::sql;
use std::collections::HashMap;
use std::fmt::Display;

mod common;

mod ack;
mod authenticate_channel;
mod change_target_consistency_states;
mod get_identities;
mod get_mirror_buddy_groups;
mod get_node_capacity_pools;
mod get_nodes;
mod get_states_and_buddy_groups;
mod get_storage_pools;
mod get_target_mappings;
mod get_target_states;
mod heartbeat;
mod heartbeat_request;
mod identity_admission;
mod map_targets;
mod map_targets_resp;
mod peer_info;
mod refresh_capacity_pools;
pub use identity_admission::create_identity_token;
pub(crate) use identity_admission::{
    Admission, AdmissionModes, KeyCategory, KeyRequest, admit_key, approve_pending_key,
    reject_pending_key,
};
mod register_identity;
pub(crate) use register_identity::{identities_version, notify_identities_changed};
mod register_node;
mod register_node_v2;
mod register_target;
mod remove_node;
mod remove_node_resp;
mod request_exceeded_quota;
mod set_channel_direct;
mod set_mirror_buddy_groups_resp;
mod set_storage_target_info;
mod set_target_consistency_states;

/// Msg request handler for requests where no response is expected.
/// To handle a message, implement this and add it to the dispatch list with `=> _`.
trait HandleNoResponse: Msg + Deserializable {
    async fn handle(self, app: &impl App, req: &mut impl Request) -> Result<()>;
}

/// Msg request handler for requests where a response is expected.
/// To handle a message, implement this and add it to the dispatch list with `=> R`.
trait HandleWithResponse: Msg + Deserializable {
    type Response: Msg + Serializable;
    async fn handle(self, app: &impl App, req: &mut impl Request) -> Result<Self::Response>;

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
pub(crate) async fn dispatch_request(app: &RuntimeApp, mut req: impl Request) -> Result<()> {
    /// Creates the dispatching match statement
    macro_rules! dispatch_msg {
        ($({$msg_type:path => $r:tt, $allow_udp:literal, $ctx_str:literal})*) => {
            // Match on the message ID provided by the request
            match req.header().msg_id() {
                $(
                    <$msg_type>::ID => {
                        if(req.is_from_udp() && !$allow_udp) {
                            log::warn!("Dropping unexpected UDP message with ID {} from {:?}",
                                req.header().msg_id(),
                                req.addr(),
                            );

                            return Ok(());
                        }

                        let des: $msg_type = req.deserialize_msg().with_context(|| {
                            format!(
                                "{} ({}) from {:?}",
                                stringify!($msg_type),
                                req.header().msg_id(),
                                req.addr()
                            )
                        })?;

                        log::trace!("INCOMING from {:?}: {:?}", req.addr(), des);

                        let res = des.handle(app, &mut req).await;
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
        {buddy_group::GetMirrorBuddyGroups => R, false, "Get buddy groups"}
        {buddy_group::GetStatesAndBuddyGroups => R, false, "Get states and buddy groups"}
        {buddy_group::SetMirrorBuddyGroupResp => _, true, "SetMirrorBuddyGroupResp"}
        {misc::Ack => _, true, "Ack"}
        {misc::AuthenticateChannel => _, false, "Authenticate connection"}
        {misc::GetIdentities => R, false, "Get identities"}
        {misc::GetNodeCapacityPools => R, false, "Get capacity pools"}
        {misc::PeerInfo => _, false, "PeerInfo"}
        {misc::RefreshCapacityPools => R, true, "Refresh capacity pools"}
        {misc::SetChannelDirect => _, false, "SetChannelDirect"}
        {node::GetNodes => R, false, "Get nodes"}
        {node::Heartbeat => R, true, "Heartbeat"}
        {node::HeartbeatRequest => R, true, "Request heartbeat"}
        {misc::RegisterIdentity => R, false, "Register identity"}
        {node::RegisterNode => R, false, "Register node"}
        {node::RegisterNodeV2 => R, false, "Register node V2"}
        {node::RemoveNode => R, false, "Remove node"}
        {node::RemoveNodeResp => _, true, "RemoveNodeResp"}
        {quota::RequestExceededQuota => R, false, "Request exceeded quota"}
        {storage_pool::GetStoragePools => R, false, "Get storage pools"}
        {target::ChangeTargetConsistencyStates => R, false, "Change target consistency states"}
        {target::GetTargetMappings => R, false, "Get target mappings"}
        {target::GetTargetStates => R, false, "Get target states"}
        {target::MapTargetsResp => _, true, "MapTargetsResp"}
        {target::MapTargets => R, false, "Map targets"}
        {target::RegisterTarget => R, false, "Register target"}
        {target::SetStorageTargetInfo => R, false, "Set storage target info"}
        {target::SetTargetConsistencyStates => R, false, "Set target consistency states"}
    }
}

async fn handle_unspecified_msg(req: impl Request) -> Result<()> {
    log::warn!(
        "Unhandled msg INCOMING from {:?} with ID {}",
        req.addr(),
        req.header().msg_id()
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

/// Fails if the management is in pre shutdown state
fn fail_on_pre_shutdown(app: &impl App) -> Result<()> {
    if app.is_pre_shutdown() {
        return Err(anyhow!(PreShutdownError {}));
    }

    Ok(())
}
