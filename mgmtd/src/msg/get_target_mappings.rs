use super::*;

pub(super) async fn handle(
    _msg: msg::GetTargetMappings,
    chn: impl RequestChannel,
    hnd: impl ComponentHandles,
) -> Result<()> {
    match hnd
        .execute_db(move |tx| db::targets::with_type(tx, NodeTypeServer::Storage))
        .await
    {
        Ok(res) => {
            chn.respond(&msg::GetTargetMappingsResp {
                mapping: res
                    .into_iter()
                    .map(|e| (e.target_id, e.node_id))
                    .collect::<HashMap<_, _>>(),
            })
            .await
        }
        Err(err) => {
            log::error!("Getting target mappings failed:\n{err:?}");
            chn.respond(&msg::GetTargetMappingsResp {
                mapping: HashMap::new(),
            })
            .await
        }
    }
}
