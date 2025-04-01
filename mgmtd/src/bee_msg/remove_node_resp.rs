use super::*;
use shared::bee_msg::node::*;

impl HandleNoResponse for RemoveNodeResp {
    async fn handle(self, _ctx: &Context, req: &mut impl Request) -> Result<()> {
        // response from server nodes to the RemoveNode notification
        log::debug!("Ignoring RemoveNodeResp msg from {:?}", req.addr());
        Ok(())
    }
}
