use super::*;
use shared::msg::ack::*;

pub(super) async fn handle(msg: Ack, _ctx: &Context, req: &impl Request) {
    log::debug!("Ignoring Ack from {:?}: ID: {:?}", req.addr(), msg.ack_id);
}
