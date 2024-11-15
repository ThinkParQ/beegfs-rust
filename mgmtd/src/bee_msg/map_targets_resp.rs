use super::*;
use shared::bee_msg::target::*;

impl HandleNoResponse for MapTargetsResp {
    async fn handle(self, _app: &impl App, _req: &mut impl Request) -> Result<()> {
        // This is sent from the nodes as a result of the MapTargets notification after
        // map_targets was called. We just ignore it.
        Ok(())
    }
}
