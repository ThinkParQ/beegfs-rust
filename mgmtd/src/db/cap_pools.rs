use super::*;
use rusqlite::named_params;

#[derive(Clone, Debug)]
pub(crate) struct EntityWithCapPool {
    pub entity_id: u16,
    pub node_id: NodeID,
    pub pool_id: StoragePoolID,
    pub cap_pool: CapacityPool,
}

macro_rules! define_for_xxx {
    ($xxx:ident, $select_entities:literal) => {
        pub(crate) fn $xxx(
            tx: &mut Transaction,
            limits: CapPoolLimits,
            dynamic_limits: Option<CapPoolDynamicLimits>,
        ) -> Result<Vec<EntityWithCapPool>> {
            select(tx, include_str!($select_entities), limits, dynamic_limits)
        }
    };
}

define_for_xxx!(for_meta_targets, "cap_pools/select_meta_targets.sql");
define_for_xxx!(for_storage_targets, "cap_pools/select_storage_targets.sql");
define_for_xxx!(
    for_meta_buddy_groups,
    "cap_pools/select_meta_buddy_groups.sql"
);
define_for_xxx!(
    for_storage_buddy_groups,
    "cap_pools/select_storage_buddy_groups.sql"
);

fn select(
    tx: &mut Transaction,
    select_entities: &str,
    limits: CapPoolLimits,
    dynamic_limits: Option<CapPoolDynamicLimits>,
) -> Result<Vec<EntityWithCapPool>> {
    let dynamic_limits = dynamic_limits.unwrap_or(CapPoolDynamicLimits {
        inodes_normal_threshold: Inodes::ZERO,
        inodes_low_threshold: Inodes::ZERO,
        space_normal_threshold: Space::ZERO,
        space_low_threshold: Space::ZERO,
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
