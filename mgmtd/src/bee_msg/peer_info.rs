use super::*;
use shared::bee_msg::misc::*;

impl HandleNoResponse for PeerInfo {
    async fn handle(self, _ctx: &Context, _req: &mut impl Request) -> Result<()> {
        // This is supposed to give some information about a connection, but it looks
        // like this isnt used at all
        Ok(())
    }
}
