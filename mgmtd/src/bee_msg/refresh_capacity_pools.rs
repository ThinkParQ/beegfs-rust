use super::*;
use shared::bee_msg::misc::*;

impl HandleWithResponse for RefreshCapacityPools {
    type Response = Ack;

    async fn handle(self, _app: &impl App, _req: &mut impl Request) -> Result<Self::Response> {
        // This message is superfluous and therefore ignored. It is meant to tell the
        // mgmtd to trigger a capacity pool pull immediately after a node starts.
        // meta and storage send a SetTargetInfo before this msg though,
        // so we handle triggering pulls there.

        Ok(Ack {
            ack_id: self.ack_id,
        })
    }
}
