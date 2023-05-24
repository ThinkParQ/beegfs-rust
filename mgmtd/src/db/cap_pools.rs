use super::*;
use rusqlite::named_params;

#[derive(Clone, Debug)]
pub(crate) struct EntityWithCapPool {
    pub entity_id: u16,
    pub node_id: NodeID,
    pub pool_id: StoragePoolID,
    pub cap_pool: CapacityPool,
}

pub(crate) fn for_meta_targets(
    tx: &mut Transaction,
    limits: CapPoolLimits,
    dynamic_limits: Option<CapPoolDynamicLimits>,
) -> Result<Vec<EntityWithCapPool>> {
    select(
        tx,
        include_str!("cap_pools/select_meta_targets.sql"),
        limits,
        dynamic_limits,
    )
}

pub(crate) fn for_storage_targets(
    tx: &mut Transaction,
    limits: CapPoolLimits,
    dynamic_limits: Option<CapPoolDynamicLimits>,
) -> Result<Vec<EntityWithCapPool>> {
    select(
        tx,
        include_str!("cap_pools/select_storage_targets.sql"),
        limits,
        dynamic_limits,
    )
}

pub(crate) fn for_meta_buddy_groups(
    tx: &mut Transaction,
    limits: CapPoolLimits,
    dynamic_limits: Option<CapPoolDynamicLimits>,
) -> Result<Vec<EntityWithCapPool>> {
    select(
        tx,
        include_str!("cap_pools/select_meta_buddy_groups.sql"),
        limits,
        dynamic_limits,
    )
}

pub(crate) fn for_storage_buddy_groups(
    tx: &mut Transaction,
    limits: CapPoolLimits,
    dynamic_limits: Option<CapPoolDynamicLimits>,
) -> Result<Vec<EntityWithCapPool>> {
    select(
        tx,
        include_str!("cap_pools/select_storage_buddy_groups.sql"),
        limits,
        dynamic_limits,
    )
}

fn select(
    tx: &mut Transaction,
    select_entities: &str,
    limits: CapPoolLimits,
    dynamic_limits: Option<CapPoolDynamicLimits>,
) -> Result<Vec<EntityWithCapPool>> {
    let dynamic_limits = dynamic_limits.unwrap_or(CapPoolDynamicLimits {
        inodes_normal_threshold: 0,
        inodes_low_threshold: 0,
        space_normal_threshold: 0,
        space_low_threshold: 0,
        inodes_low: limits.inodes_low,
        inodes_emergency: limits.inodes_emergency,
        space_low: limits.space_low,
        space_emergency: limits.space_emergency,
    });

    let mut stmt = tx.prepare_cached(&format!(
        include_str!("cap_pools/select.sql"),
        select_entities = select_entities
    ))?;

    let cap_pools = stmt
        .query_map(
            named_params![
                ":space_low_limit": limits.space_low,
                ":space_em_limit": limits.space_emergency,
                ":inodes_low_limit": limits.inodes_low,
                ":inodes_em_limit": limits.inodes_emergency,
                ":space_normal_threshold": dynamic_limits.space_normal_threshold,
                ":space_low_threshold": dynamic_limits.space_low_threshold,
                ":inodes_normal_threshold": dynamic_limits.inodes_normal_threshold,
                ":inodes_low_threshold": dynamic_limits.inodes_low_threshold,
                ":space_low_dynamic_limit": dynamic_limits.space_low,
                ":space_em_dynamic_limit": dynamic_limits.space_emergency,
                ":inodes_low_dynamic_limit": dynamic_limits.inodes_low,
                ":inodes_em_dynamic_limit": dynamic_limits.inodes_emergency,
            ],
            |row| {
                Ok(EntityWithCapPool {
                    entity_id: row.get(0)?,
                    node_id: row.get(1)?,
                    pool_id: row.get(2)?,
                    cap_pool: match row.get_ref(3)?.as_str()? {
                        "normal" => CapacityPool::Normal,
                        "low" => CapacityPool::Low,
                        _ => CapacityPool::Emergency,
                    },
                })
            },
        )?
        .try_collect()?;

    Ok(cap_pools)
}
