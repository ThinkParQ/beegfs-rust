use log::warn;
use shared::msg::*;
use shared::types::*;

pub(crate) fn default_response(msg: Generic) -> Option<Generic> {
    match msg.msg_id() {
        // TCP
        SetChannelDirect::ID => None,
        SetExceededQuota::ID => Some(Generic::from_beemsg(SetExceededQuotaResp {
            result: OpsErr::SUCCESS,
        })),
        RemoveBuddyGroup::ID => Some(Generic::from_beemsg(RemoveBuddyGroupResp {
            result: OpsErr::SUCCESS,
        })),
        // UDP
        Ack::ID => None,
        Heartbeat::ID => {
            let msg: Heartbeat = msg.into_beemsg();
            Some(Generic::from_beemsg(Ack { ack_id: msg.ack_id }))
        }
        RefreshCapacityPools::ID => {
            let msg: RefreshCapacityPools = msg.into_beemsg();
            Some(Generic::from_beemsg(Ack { ack_id: msg.ack_id }))
        }
        RefreshTargetStates::ID => {
            let msg: RefreshTargetStates = msg.into_beemsg();
            Some(Generic::from_beemsg(Ack { ack_id: msg.ack_id }))
        }
        RefreshStoragePools::ID => {
            let msg: RefreshStoragePools = msg.into_beemsg();
            Some(Generic::from_beemsg(Ack { ack_id: msg.ack_id }))
        }
        MapTargets::ID => {
            let msg: MapTargets = msg.into_beemsg();
            Some(Generic::from_beemsg(Ack { ack_id: msg.ack_id }))
        }
        PublishCapacities::ID => {
            let msg: PublishCapacities = msg.into_beemsg();
            Some(Generic::from_beemsg(Ack { ack_id: msg.ack_id }))
        }
        m => {
            warn!("Unhandled msg: {m}");
            None
        }
    }
}
