use super::*;

pub(super) async fn handle(
    _msg: msg::GetTargetMappings,
    ctx: &impl AppContext,
    _req: &impl Request,
) -> msg::GetTargetMappingsResp {
    match ctx
        .db_op(move |tx| db::target::get_with_type(tx, NodeTypeServer::Storage))
        .await
    {
        Ok(res) => msg::GetTargetMappingsResp {
            mapping: res
                .into_iter()
                .map(|e| (e.target_id, e.node_id))
                .collect::<HashMap<_, _>>(),
        },
        Err(err) => {
            log_error_chain!(err, "Getting target mappings failed");
            msg::GetTargetMappingsResp {
                mapping: HashMap::new(),
            }
        }
    }
}
