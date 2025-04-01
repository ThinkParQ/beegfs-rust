use super::*;
use db::node_nic::map_bee_msg_nics;
use shared::bee_msg::node::Heartbeat;

/// Sets the entity alias for any entity
pub(crate) async fn set_alias(
    app: &impl App,
    req: pm::SetAliasRequest,
) -> Result<pm::SetAliasResponse> {
    fail_on_pre_shutdown(app)?;

    // Parse proto msg
    let entity_type: EntityType = req.entity_type().try_into()?;
    let entity_id: EntityId = required_field(req.entity_id)?.try_into()?;
    let new_alias: Alias = req.new_alias.try_into()?;

    let update_alias_fn = move |tx: &Transaction, new_alias: &Alias| -> Result<EntityIdSet> {
        let entity = entity_id.resolve(tx, entity_type)?;

        if entity.node_type() == NodeType::Client {
            bail!("Client updates are not supported")
        }

        // Check that the alias is not in use yet
        let et: Option<EntityType> = tx
            .query_row_cached(
                sql!("SELECT entity_type FROM entities WHERE alias = ?1"),
                [new_alias.as_ref()],
                |row| EntityType::from_row(row, 0),
            )
            .optional()?;

        if let Some(et) = et {
            bail!(
                "Alias {} is already in use by a {}",
                new_alias,
                et.user_str()
            );
        }

        tx.execute_cached(
            sql!("UPDATE entities SET alias = ?1 WHERE uid = ?2"),
            params![new_alias.as_ref(), entity.uid],
        )?;

        Ok(entity)
    };

    // If the entity is a node, notify all nodes about the changed alias
    if entity_type == EntityType::Node {
        let (entity, node, nic_list) = app
            .write_tx(move |tx| {
                let entity = update_alias_fn(tx, &new_alias)?;

                let node = db::node::get_by_alias(tx, new_alias.as_ref())?;
                let nic_list = db::node_nic::get_with_node(tx, entity.uid)?;

                Ok((entity, node, nic_list))
            })
            .await?;

        app.send_notifications(
            &[NodeType::Meta, NodeType::Storage, NodeType::Client],
            &Heartbeat {
                instance_version: 0,
                nic_list_version: 0,
                node_type: entity.node_type(),
                node_alias: node.alias.into_bytes(),
                ack_id: "".into(),
                node_num_id: entity.num_id(),
                root_num_id: 0,
                is_root_mirrored: 0,
                port: node.port,
                port_tcp_unused: node.port,
                nic_list: map_bee_msg_nics(nic_list).collect(),
                machine_uuid: vec![],
            },
        )
        .await;

    // If not a node, just update the alias
    } else {
        app.write_tx(move |tx| update_alias_fn(tx, &new_alias))
            .await?;
    }

    Ok(pm::SetAliasResponse {})
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::app::test::*;

    #[tokio::test]
    async fn set_alias() {
        let app = TestApp::new().await;

        // Nonexisting entity
        super::set_alias(
            &app,
            pm::SetAliasRequest {
                entity_id: Some(EntityId::Uid(99999999).into()),
                entity_type: pb::EntityType::Node.into(),
                new_alias: "new_alias".to_string(),
            },
        )
        .await
        .unwrap_err();

        // Invalid entity_type / entity_id combination
        super::set_alias(
            &app,
            pm::SetAliasRequest {
                entity_id: Some(EntityId::Uid(101001).into()),
                entity_type: pb::EntityType::Target.into(),
                new_alias: "new_alias".to_string(),
            },
        )
        .await
        .unwrap_err();

        // Alias already in use
        super::set_alias(
            &app,
            pm::SetAliasRequest {
                entity_id: Some(EntityId::Alias("meta_node_1".try_into().unwrap()).into()),
                entity_type: pb::EntityType::Node.into(),
                new_alias: "meta_node_2".to_string(),
            },
        )
        .await
        .unwrap_err();

        // Deny setting client aliases
        super::set_alias(
            &app,
            pm::SetAliasRequest {
                entity_id: Some(
                    EntityId::LegacyID(LegacyId {
                        node_type: NodeType::Client,
                        num_id: 1,
                    })
                    .into(),
                ),
                entity_type: pb::EntityType::Node.into(),
                new_alias: "new_alias".to_string(),
            },
        )
        .await
        .unwrap_err();

        // Success
        super::set_alias(
            &app,
            pm::SetAliasRequest {
                entity_id: Some(EntityId::Uid(101001).into()),
                entity_type: pb::EntityType::Node.into(),
                new_alias: "new_alias".to_string(),
            },
        )
        .await
        .unwrap();

        assert!(app.has_sent_notification::<Heartbeat>(&[
            NodeType::Meta,
            NodeType::Storage,
            NodeType::Client,
        ]));

        assert_eq_db!(
            app,
            "SELECT alias FROM entities WHERE uid = ?1",
            [101001],
            "new_alias"
        );
    }
}
