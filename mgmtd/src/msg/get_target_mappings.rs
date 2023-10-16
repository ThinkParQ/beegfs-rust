use super::*;
use shared::msg::get_target_mappings::{GetTargetMappings, GetTargetMappingsResp};
use shared::types::NodeTypeServer;

pub(super) async fn handle(
    _msg: GetTargetMappings,
    ctx: &Context,
    _req: &impl Request,
) -> GetTargetMappingsResp {
    match ctx
        .db
        .op(move |tx| db::target::get_with_type(tx, NodeTypeServer::Storage))
        .await
    {
        Ok(res) => GetTargetMappingsResp {
            mapping: res
                .into_iter()
                .map(|e| (e.target_id, e.node_id))
                .collect::<HashMap<_, _>>(),
        },
        Err(err) => {
            log_error_chain!(err, "Getting target mappings failed");
            GetTargetMappingsResp {
                mapping: HashMap::new(),
            }
        }
    }
}
