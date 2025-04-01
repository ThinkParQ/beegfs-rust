use super::*;
use shared::bee_msg::misc::*;

impl HandleNoResponse for Ack {
    async fn handle(self, _ctx: &Context, req: &mut impl Request) -> Result<()> {
        log::debug!("Ignoring Ack from {:?}: Id: {:?}", req.addr(), self.ack_id);
        Ok(())
    }
}
