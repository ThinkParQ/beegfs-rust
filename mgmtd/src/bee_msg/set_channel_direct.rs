use super::*;
use shared::bee_msg::misc::*;

impl HandleNoResponse for SetChannelDirect {
    async fn handle(self, _ctx: &Context, _req: &mut impl Request) -> Result<()> {
        // do nothing
        Ok(())
    }
}
