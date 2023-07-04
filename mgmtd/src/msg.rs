use crate::config::DynamicConfig;
use crate::db;
use crate::db::{DbError, DbResult};
use crate::notification::Notification;
use anyhow::{bail, Result};
use async_trait::async_trait;
use rusqlite::Transaction;
use shared::conn::msg_dispatch::*;
use shared::conn::PeerID;
use shared::msg::{self, GenericResponse, Msg};
use shared::*;
use std::collections::HashMap;
use std::sync::RwLockReadGuard;

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
mod register_storage_target;
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
mod set_target_consistency_states;
mod set_target_info;
mod unmap_target;

// TODO put the config source requirement into the trait
#[async_trait]
pub(crate) trait ComponentInteractor: Clone {
    async fn db_op<T: Send + 'static + FnOnce(&mut Transaction) -> DbResult<R>, R: Send + 'static>(
        &self,
        op: T,
    ) -> DbResult<R>;

    async fn request<M: Msg, R: Msg>(&self, dest: PeerID, msg: &M) -> Result<R, anyhow::Error>;
    async fn send<M: Msg>(&self, dest: PeerID, msg: &M) -> Result<(), anyhow::Error>;
    async fn notify_nodes<M: Notification<'static>>(&self, msg: &M);

    fn get_config(&self) -> RwLockReadGuard<DynamicConfig>;
    fn get_static_info(&self) -> &'static crate::StaticInfo;
}

pub(crate) async fn dispatch_request(
    ci: impl ComponentInteractor,
    mut req: impl RequestConnectionController + DeserializeMsg,
) -> anyhow::Result<()> {
    macro_rules! dispatch_msg {
        ($($msg_type:path => $submod:ident),*) => {
            match req.msg_id() {
                $(
                    <$msg_type>::ID => {
                        let des: $msg_type = req.deserialize_msg()?;
                        log::debug!("INCOMING from {:?}: {:?}", req.peer(), des);

                        #[allow(clippy::unnecessary_mut_passed)]
                        let response = $submod::handle(des, ci, &mut req).await;

                        log::debug!("PROCESSED from {:?}. Responding: {:?}", req.peer(), response);
                        ResponseMsg::respond(req, &response).await
                    }
                ),*

                id => {
                    log::warn!("UNHANDLED INCOMING from {:?} with ID {id}", req.peer());

                    ResponseMsg::respond(req, &GenericResponse {
                        code: GenericResponseCode::TRY_AGAIN,
                        description: "Unhandled msg".into(),
                    }).await?;

                    Ok(())
                }
            }
        }
    }

    dispatch_msg!(
        // TCP
        msg::RegisterNode => register_node,
        msg::RemoveNode => remove_node,
        msg::GetNodes => get_nodes,
        msg::RegisterStorageTarget => register_storage_target,
        msg::MapTargets => map_targets,
        msg::GetTargetMappings => get_target_mappings,
        msg::GetTargetStates => get_target_states,
        msg::GetStoragePools => get_storage_pools,
        msg::GetStatesAndBuddyGroups => get_states_and_buddy_groups,
        msg::GetNodeCapacityPools => get_node_capacity_pools,
        msg::ChangeTargetConsistencyStates => change_target_consistency_states,
        msg::SetTargetInfo => set_target_info,
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
        msg::SetMirrorBuddyGroupResp => set_mirror_buddy_group_resp
    )
}
