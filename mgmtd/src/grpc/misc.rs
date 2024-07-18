use super::*;
use shared::bee_msg::node::*;

/// Sets the entity alias for any entity
pub(crate) async fn set_alias(
    ctx: &Context,
    req: pm::SetAliasRequest,
) -> Result<pm::SetAliasResponse> {
    // Parse proto msg
    let entity_type: EntityType = req.entity_type().try_into()?;
    let entity_id: EntityId = required_field(req.entity_id)?.try_into()?;
    let alias: Alias = req.new_alias.try_into()?;

    let db_res = ctx
        .db
        .op(move |tx| {
            let entity = entity_id.resolve(tx, entity_type)?;

            if entity.node_type() == &NodeType::Client {
                bail!("Client updates are not supported")
            }

            // Check that the alias is not in use yet
            let et: Option<EntityType> = tx
                .query_row_cached(
                    sql!("SELECT entity_type FROM entities WHERE alias = ?1"),
                    [alias.as_ref()],
                    |row| EntityType::from_row(row, 0),
                )
                .optional()?;

            if let Some(et) = et {
                bail!("Alias {} is already in use by a {}", alias, et.user_str());
            }

            tx.execute_cached(
                sql!("UPDATE entities SET alias = ?1 WHERE uid = ?2"),
                params![alias.as_ref(), entity.uid],
            )?;

            let node = db::node::get_by_alias(tx, alias.as_ref())?;
            let node_nics = db::node_nic::get_with_node(tx, node.uid)?;

            Ok((node, node_nics))
        })
        .await?;

    // notify all nodes
    notify_nodes(
        ctx,
        &[NodeType::Meta, NodeType::Storage, NodeType::Client],
        &Heartbeat {
            instance_version: 0,
            nic_list_version: 0,
            node_type: db_res.0.node_type,
            node_alias: db_res.0.alias.into_bytes(),
            ack_id: "".into(),
            node_num_id: db_res.0.id,
            root_num_id: 0,
            is_root_mirrored: 0,
            port: db_res.0.port,
            port_tcp_unused: db_res.0.port,
            nic_list: db_res
                .1
                .into_iter()
                .map(|e| Nic {
                    addr: e.addr,
                    name: e.name.into_bytes(),
                    nic_type: e.nic_type,
                })
                .collect(),
        },
    )
    .await;

    Ok(pm::SetAliasResponse {})
}
