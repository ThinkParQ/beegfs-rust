use super::*;
use shared::msg::types::{QuotaInodeSupport, QuotaQueryType};

pub(super) async fn handle(
    msg: msg::GetQuotaInfo,
    ctx: &Context,
    _req: &impl Request,
) -> msg::GetQuotaInfoResp {
    // TODO Respect the requested transfer method. Or, at least, query by target, not by storage
    // pool (since this and only this is used by ctl for the request).

    let pool_id = msg.pool_id;

    match ctx
        .db
        .op(move |tx| {
            // Check pool id exists
            if db::storage_pool::get_uid(tx, msg.pool_id)?.is_none() {
                bail!(TypedError::value_not_found("storage pool ID", msg.pool_id));
            }

            let limits = match msg.query_type {
                QuotaQueryType::None => return Ok(vec![]),
                QuotaQueryType::Single => db::quota_limit::with_quota_id_range(
                    tx,
                    msg.id_range_start..=msg.id_range_start,
                    msg.pool_id,
                    msg.id_type,
                )?,
                QuotaQueryType::Range => db::quota_limit::with_quota_id_range(
                    tx,
                    msg.id_range_start..=msg.id_range_end,
                    msg.pool_id,
                    msg.id_type,
                )?,
                QuotaQueryType::List => {
                    db::quota_limit::with_quota_id_list(tx, msg.id_list, msg.pool_id, msg.id_type)?
                }
                QuotaQueryType::All => {
                    // This is actually unused on the old ctl side, if --all is provided, it sends a
                    // list
                    db::quota_limit::all(tx, msg.pool_id, msg.id_type)?
                }
            };

            let res = limits
                .into_iter()
                .map(|limit| msg::types::QuotaEntry {
                    space: limit.space.unwrap_or_default(),
                    inodes: limit.inodes.unwrap_or_default(),
                    id: limit.quota_id,
                    id_type: msg.id_type,
                    valid: 1,
                })
                .collect();

            Ok(res)
        })
        .await
    {
        Ok(data) => msg::GetQuotaInfoResp {
            quota_inode_support: QuotaInodeSupport::Unknown,
            quota_entry: data,
        },
        Err(err) => {
            log_error_chain!(
                err,
                "Getting quota info for storage pool {} failed",
                pool_id,
            );

            msg::GetQuotaInfoResp {
                quota_inode_support: QuotaInodeSupport::Unknown,
                quota_entry: vec![],
            }
        }
    }
}
