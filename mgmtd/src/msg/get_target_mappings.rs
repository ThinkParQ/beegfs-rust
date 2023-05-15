use super::*;

pub(super) async fn handle(
    _msg: msg::GetTargetMappings,
    rcc: impl RequestConnectionController,
    ci: impl ComponentInteractor,
) -> Result<()> {
    match ci
        .execute_db(move |tx| db::targets::with_type(tx, NodeTypeServer::Storage))
        .await
    {
        Ok(res) => {
            rcc.respond(&msg::GetTargetMappingsResp {
                mapping: res
                    .into_iter()
                    .map(|e| (e.target_id, e.node_id))
                    .collect::<HashMap<_, _>>(),
            })
            .await
        }
        Err(err) => {
            log::error!("Getting target mappings failed:\n{err:?}");
            rcc.respond(&msg::GetTargetMappingsResp {
                mapping: HashMap::new(),
            })
            .await
        }
    }
}
