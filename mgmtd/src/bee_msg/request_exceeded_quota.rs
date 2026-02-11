use super::*;
use rusqlite::params;
use shared::bee_msg::quota::*;

impl HandleWithResponse for RequestExceededQuota {
    type Response = RequestExceededQuotaResp;

    fn error_response() -> Self::Response {
        RequestExceededQuotaResp {
            result: OpsErr::INTERNAL,
            inner: SetExceededQuota::default(),
        }
    }

    async fn handle(self, app: &impl App, _req: &mut impl Request) -> Result<Self::Response> {
        let inner = app
            .read_tx(move |tx| {
                // Quota is calculated per pool, so if a target ID is given, use its assigned pools
                // ID.
                let pool_id = if self.pool_id != 0 {
                    self.pool_id
                } else {
                    tx.query_row_cached(
                        sql!("SELECT pool_id FROM storage_targets WHERE target_id = ?1"),
                        [self.target_id],
                        |row| row.get(0),
                    )?
                };

                let exceeded_quota_ids = tx.query_map_collect(
                    sql!(
                        "SELECT DISTINCT e.quota_id FROM quota_usage AS e
                        INNER JOIN targets AS st USING(node_type, target_id)
                        LEFT JOIN quota_default_limits AS d USING(id_type, quota_type, pool_id)
                        LEFT JOIN quota_limits AS l USING(quota_id, id_type, quota_type, pool_id)
                        WHERE e.id_type = ?1 AND e.quota_type = ?2 AND st.pool_id = ?3
                        GROUP BY e.quota_id, e.id_type, e.quota_type, st.pool_id
                        HAVING SUM(e.value) > COALESCE(l.value, d.value)"
                    ),
                    params![
                        self.id_type.sql_variant(),
                        self.quota_type.sql_variant(),
                        pool_id
                    ],
                    |row| row.get(0),
                )?;

                Ok(SetExceededQuota {
                    pool_id: self.pool_id,
                    id_type: self.id_type,
                    quota_type: self.quota_type,
                    exceeded_quota_ids,
                })
            })
            .await?;

        Ok(RequestExceededQuotaResp {
            result: OpsErr::SUCCESS,
            inner,
        })
    }
}
