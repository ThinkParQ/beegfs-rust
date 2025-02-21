use super::SqliteEnumExt;
use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, Params, Transaction, params};
use shared::types::*;
use sqlite::TransactionExt;
use sqlite_check::sql;

pub(crate) trait ResolveEntityId: std::fmt::Display {
    fn try_resolve(&self, tx: &Transaction, entity_type: EntityType)
    -> Result<Option<EntityIdSet>>;

    fn resolve(&self, tx: &Transaction, entity_type: EntityType) -> Result<EntityIdSet> {
        let res = self.try_resolve(tx, entity_type)?;

        let Some(res) = res else {
            bail!("{} {} does not exist", entity_type, self);
        };

        Ok(res)
    }
}

impl ResolveEntityId for EntityId {
    fn try_resolve(
        &self,
        tx: &Transaction,
        entity_type: EntityType,
    ) -> Result<Option<EntityIdSet>> {
        match self {
            EntityId::Alias(alias) => alias.try_resolve(tx, entity_type),
            EntityId::LegacyID(legacy_id) => legacy_id.try_resolve(tx, entity_type),
            EntityId::Uid(uid) => uid.try_resolve(tx, entity_type),
        }
    }
}

impl ResolveEntityId for Uid {
    fn try_resolve(
        &self,
        tx: &Transaction,
        entity_type: EntityType,
    ) -> Result<Option<EntityIdSet>> {
        let sql = resolve_sql_from(entity_type);
        let sql = format!("{sql} WHERE uid = ?1");
        resolve_query(tx, &sql, [self])
    }
}

impl ResolveEntityId for LegacyId {
    fn try_resolve(
        &self,
        tx: &Transaction,
        entity_type: EntityType,
    ) -> Result<Option<EntityIdSet>> {
        let sql = resolve_sql_from(entity_type);
        let sql = format!("{sql} WHERE node_type = ?1 AND id = ?2");
        resolve_query(tx, &sql, params![self.node_type.sql_variant(), self.num_id])
    }
}

pub(crate) fn try_resolve_num_id(
    tx: &Transaction,
    entity_type: EntityType,
    node_type: NodeType,
    num_id: u32,
) -> Result<Option<EntityIdSet>> {
    LegacyId { node_type, num_id }.try_resolve(tx, entity_type)
}

pub(crate) fn resolve_num_id(
    tx: &Transaction,
    entity_type: EntityType,
    node_type: NodeType,
    num_id: u32,
) -> Result<EntityIdSet> {
    LegacyId { node_type, num_id }.resolve(tx, entity_type)
}

impl ResolveEntityId for Alias {
    fn try_resolve(
        &self,
        tx: &Transaction,
        entity_type: EntityType,
    ) -> Result<Option<EntityIdSet>> {
        let sql = resolve_sql_from(entity_type);
        let sql = format!("{sql} WHERE alias = ?1");
        resolve_query(tx, &sql, [self.as_ref()])
    }
}

fn resolve_sql_from(entity_type: EntityType) -> &'static str {
    match entity_type {
        EntityType::Node => {
            sql!("SELECT node_uid AS uid, alias, node_type, node_id AS id FROM nodes_ext")
        }
        EntityType::Target => {
            sql!("SELECT target_uid AS uid, alias, node_type, target_id AS id FROM targets_ext")
        }
        EntityType::BuddyGroup => {
            sql!("SELECT group_uid AS uid, alias, node_type, group_id AS id FROM buddy_groups_ext")
        }
        EntityType::Pool => {
            sql!("SELECT pool_uid AS uid, alias, 2 AS node_type, pool_id AS id FROM pools_ext")
        }
    }
}

fn resolve_query(tx: &Transaction, sql: &str, params: impl Params) -> Result<Option<EntityIdSet>> {
    let res = tx
        .query_row_cached(sql, params, |row| {
            Ok((
                row.get::<_, Uid>(0)?,
                row.get::<_, String>(1)?,
                NodeType::from_row(row, 2)?,
                row.get::<_, u32>(3)?,
            ))
        })
        .optional()?;

    if let Some(res) = res {
        Ok(Some(EntityIdSet {
            uid: res.0,
            alias: res.1.try_into()?,
            legacy_id: LegacyId {
                node_type: res.2,
                num_id: res.3,
            },
        }))
    } else {
        Ok(None)
    }
}
