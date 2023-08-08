use super::*;

pub(super) async fn handle(msg: msg::Ack, _ctx: &impl AppContext, req: &impl Request) {
    log::debug!("Ignoring Ack from {:?}: ID: {:?}", req.addr(), msg.ack_id);
}
