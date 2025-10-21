use super::*;
use shared::bee_msg::buddy_group::*;

impl HandleNoResponse for SetMirrorBuddyGroupResp {
    async fn handle(self, _app: &impl App, _req: &mut impl Request) -> Result<()> {
        // response from server nodes to SetMirrorBuddyGroup notification
        Ok(())
    }
}
