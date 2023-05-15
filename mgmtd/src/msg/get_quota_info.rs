use super::*;
use shared::msg::types::{QuotaInodeSupport, QuotaQueryType};

pub(super) async fn handle(
    msg: msg::GetQuotaInfo,
    rcc: impl RequestConnectionController,
    ci: impl ComponentInteractor,
) -> Result<()> {
    let pool_id = msg.pool_id;

    match ci
        .execute_db(move |tx| {
            let limits = match msg.query_type {
                QuotaQueryType::None => return Ok(vec![]),
                QuotaQueryType::Single => vec![db::quota_limits::with_quota_id(
                    tx,
                    msg.id_range_start,
                    msg.pool_id,
                    msg.id_type,
                )?],
                QuotaQueryType::Range => db::quota_limits::with_quota_id_range(
                    tx,
                    msg.id_range_start..=msg.id_range_end,
                    msg.pool_id,
                    msg.id_type,
                )?,
                QuotaQueryType::List => {
                    db::quota_limits::with_quota_id_list(tx, msg.id_list, msg.pool_id, msg.id_type)?
                }
                QuotaQueryType::All => {
                    // This is actually unused on the old ctl side, if --all is provided, it sends a
                    // list
                    db::quota_limits::all(tx, msg.pool_id, msg.id_type)?
                }
            };

            let res = limits
                .into_iter()
                .map(|limit| msg::types::QuotaEntry {
                    space: limit.space.unwrap_or_default(),
                    inodes: limit.inodes.unwrap_or_default(),
                    id: limit.quota_id,
                    id_type: msg.id_type,
                    valid: true,
                })
                .collect();

            Ok(res)
        })
        .await
    {
        Ok(data) => {
            rcc.respond(&msg::GetQuotaInfoResp {
                quota_inode_support: QuotaInodeSupport::Unknown,
                quota_entry: data,
            })
            .await
        }
        Err(err) => {
            log::error!(
                "Getting quota info for storage pool {} failed:\n{:?}",
                pool_id,
                err
            );

            rcc.respond(&msg::GetQuotaInfoResp {
                quota_inode_support: QuotaInodeSupport::Unknown,
                quota_entry: vec![],
            })
            .await
        }
    }
}
