use super::*;
use pm::get_pending_keys_response::PendingKey;
use pm::get_pending_keys_response::pending_key::Category;

pub(crate) async fn get_pending_keys(
    app: &impl App,
    _req: pm::GetPendingKeysRequest,
) -> Result<pm::GetPendingKeysResponse> {
    fail_on_legacy(app)?;

    let keys = app
        .read_tx(|tx| {
            Ok(tx.query_map_collect(
                sql!(
                    "SELECT key, category, node_type, node_id, name, addr, rejected,
                        CAST(STRFTIME('%s', first_seen) AS INTEGER),
                        CAST(STRFTIME('%s', last_seen) AS INTEGER)
                    FROM pending_keys
                    ORDER BY first_seen, key"
                ),
                [],
                |row| {
                    let category = match row.get_ref(1)?.as_str()? {
                        "client" => Category::Client,
                        "server" => Category::Server,
                        _ => Category::Unspecified,
                    };
                    let node_type = row
                        .get::<_, Option<i64>>(2)?
                        .map(NodeType::from_sql_variant)
                        .transpose()?
                        .map(|t| t.into_proto_i32());

                    Ok(PendingKey {
                        key_id: Some(row.get(0)?),
                        category: Some(category.into()),
                        node_type,
                        node_id: row.get(3)?,
                        name: row.get(4)?,
                        addr: Some(row.get(5)?),
                        rejected: Some(row.get(6)?),
                        first_seen_unix: Some(row.get(7)?),
                        last_seen_unix: Some(row.get(8)?),
                    })
                },
            )?)
        })
        .await?;

    Ok(pm::GetPendingKeysResponse { keys })
}
