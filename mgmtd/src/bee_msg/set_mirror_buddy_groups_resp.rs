use super::*;
use shared::bee_msg::buddy_group::*;

impl HandleNoResponse for SetMirrorBuddyGroupResp {
    async fn handle(self, _ctx: &Context, _req: &mut impl Request) -> Result<()> {
        // response from server nodes to SetMirrorBuddyGroup notification
        Ok(())
    }
}
