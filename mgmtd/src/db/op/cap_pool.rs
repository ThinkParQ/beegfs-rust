//! Functions for calculating targets and buddy groups capacity pools.
//!
//! Pools are calculated based on the behavior in old management.
//!
//! All of the functions expect the pool limit configuration to be put in from the caller. The
//! `dynamic_limits` are optional and will be ignored if not given (e.g. when disabled by config).
//!
//! # Return value
//! The functions return a Vec with the [EntityWithCapPool] result struct, mapping targets / buddy
//! groups to capacity pools. When retrieving meta capacity pools, `pool_id` is always `0`.

use super::*;
use rusqlite::named_params;
use std::borrow::Cow;

/// The result entry, assigning a target to a capacity pool.
///
/// Also contains additional information which is useful for the caller.
#[derive(Clone, Debug)]
pub struct EntityWithCapPool {
    pub entity_id: u16,
    pub node_id: NodeID,
    pub pool_id: StoragePoolID,
    pub cap_pool: CapacityPool,
}

/// Calculate capacity pools for meta targets
pub fn for_meta_targets(
    tx: &mut Transaction,
    cap_pool_meta_limits: &CapPoolLimits,
    cap_pool_dynamic_meta_limits: Option<&CapPoolDynamicLimits>,
) -> Result<Vec<EntityWithCapPool>> {
    select(
        tx,
        include_str!("cap_pool/select_meta_targets.sql"),
        cap_pool_meta_limits,
        cap_pool_dynamic_meta_limits,
    )
}

/// Calculate capacity pools for storage targets
pub fn for_storage_targets(
    tx: &mut Transaction,
    cap_pool_storage_limits: &CapPoolLimits,
    cap_pool_dynamic_storage_limits: Option<&CapPoolDynamicLimits>,
) -> Result<Vec<EntityWithCapPool>> {
    select(
        tx,
        include_str!("cap_pool/select_storage_targets.sql"),
        cap_pool_storage_limits,
        cap_pool_dynamic_storage_limits,
    )
}

/// Calculate capacity pools for meta buddy groups
pub fn for_meta_buddy_groups(
    tx: &mut Transaction,
    cap_pool_meta_limits: &CapPoolLimits,
    cap_pool_dynamic_meta_limits: Option<&CapPoolDynamicLimits>,
) -> Result<Vec<EntityWithCapPool>> {
    select(
        tx,
        include_str!("cap_pool/select_meta_buddy_groups.sql"),
        cap_pool_meta_limits,
        cap_pool_dynamic_meta_limits,
    )
}

/// Calculate capacity pools for storage buddy groups
pub fn for_storage_buddy_groups(
    tx: &mut Transaction,
    cap_pool_storage_limits: &CapPoolLimits,
    cap_pool_dynamic_storage_limits: Option<&CapPoolDynamicLimits>,
) -> Result<Vec<EntityWithCapPool>> {
    select(
        tx,
        include_str!("cap_pool/select_storage_buddy_groups.sql"),
        cap_pool_storage_limits,
        cap_pool_dynamic_storage_limits,
    )
}

/// Execute the actual select statement to fetch and calculate capacity pools from DB.
///
/// Requires type specific part of the statement (`select_entities`) and type specific config
/// parameters (`limits` and `dynamic_limits`)
fn select(
    tx: &mut Transaction,
    select_entities: &str,
    limits: &CapPoolLimits,
    dynamic_limits: Option<&CapPoolDynamicLimits>,
) -> Result<Vec<EntityWithCapPool>> {
    let dynamic_limits = match dynamic_limits {
        Some(dl) => Cow::Borrowed(dl),
        None => Cow::Owned(CapPoolDynamicLimits {
            inodes_normal_threshold: 0,
            inodes_low_threshold: 0,
            space_normal_threshold: 0,
            space_low_threshold: 0,
            inodes_low: limits.inodes_low,
            inodes_emergency: limits.inodes_emergency,
            space_low: limits.space_low,
            space_emergency: limits.space_emergency,
        }),
    };

    let mut stmt = tx.prepare_cached(&format!(
        include_str!("cap_pool/select.sql"),
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
                    cap_pool: row.get(3)?,
                })
            },
        )?
        .try_collect()?;

    Ok(cap_pools)
}

#[cfg(test)]
mod test {
    use super::*;

    const FIXED_LIMITS: &[(CapPoolLimits, CapacityPool)] = &[
        (
            CapPoolLimits {
                inodes_low: 800000,
                inodes_emergency: 600000,
                space_low: 200000,
                space_emergency: 100000,
            },
            CapacityPool::Emergency,
        ),
        (
            CapPoolLimits {
                inodes_low: 100000,
                inodes_emergency: 200000,
                space_low: 800000,
                space_emergency: 600000,
            },
            CapacityPool::Emergency,
        ),
        (
            CapPoolLimits {
                inodes_low: 100000,
                inodes_emergency: 200000,
                space_low: 800000,
                space_emergency: 400000,
            },
            CapacityPool::Low,
        ),
        (
            CapPoolLimits {
                inodes_low: 200000,
                inodes_emergency: 100000,
                space_low: 200000,
                space_emergency: 100000,
            },
            CapacityPool::Normal,
        ),
    ];

    #[test]
    fn fixed_limits_for_meta_targets() {
        with_test_data(|tx| {
            for (i, set) in FIXED_LIMITS.iter().enumerate() {
                let pools = for_meta_targets(tx, &set.0, None).unwrap();
                assert!(pools.into_iter().all(|e| e.cap_pool == set.1), "Case #{i}");
            }
        })
    }

    #[test]
    fn fixed_limits_for_meta_buddy_groups() {
        with_test_data(|tx| {
            for (i, set) in FIXED_LIMITS.iter().enumerate() {
                let pools = for_meta_buddy_groups(tx, &set.0, None).unwrap();
                assert!(pools.into_iter().all(|e| e.cap_pool == set.1), "Case #{i}");
            }
        })
    }

    #[test]
    fn fixed_limits_for_storage_targets() {
        with_test_data(|tx| {
            for (i, set) in FIXED_LIMITS.iter().enumerate() {
                let pools = for_storage_targets(tx, &set.0, None).unwrap();
                assert!(pools.into_iter().all(|e| e.cap_pool == set.1), "Case #{i}");
            }
        })
    }

    #[test]
    fn fixed_limits_for_storage_buddy_groups() {
        with_test_data(|tx| {
            for (i, set) in FIXED_LIMITS.iter().enumerate() {
                let pools = for_storage_buddy_groups(tx, &set.0, None).unwrap();
                assert!(pools.into_iter().all(|e| e.cap_pool == set.1), "Case #{i}");
            }
        })
    }

    const DYNAMIC_LIMITS: &[(CapPoolLimits, CapPoolDynamicLimits, CapacityPool)] = &[
        // All test targets in Normal pool, dynamic limits raise inode low limit
        // => all targets in Low pool
        (
            CapPoolLimits {
                inodes_low: 200000,
                inodes_emergency: 100000,
                space_low: 200000,
                space_emergency: 100000,
            },
            CapPoolDynamicLimits {
                inodes_normal_threshold: 50000,
                inodes_low_threshold: 999999,
                space_normal_threshold: 999999,
                space_low_threshold: 999999,
                inodes_low: 600000,
                inodes_emergency: 100000,
                space_low: 200000,
                space_emergency: 100000,
            },
            CapacityPool::Low,
        ),
        // All test targets in Low pool (due to space), dynamic limits raise space emergency limit
        // => all targets in Emergency pool
        (
            CapPoolLimits {
                inodes_low: 200000,
                inodes_emergency: 100000,
                space_low: 800000,
                space_emergency: 100000,
            },
            CapPoolDynamicLimits {
                inodes_normal_threshold: 999999,
                inodes_low_threshold: 999999,
                space_normal_threshold: 999999,
                space_low_threshold: 50000,
                inodes_low: 200000,
                inodes_emergency: 100000,
                space_low: 800000,
                space_emergency: 600000,
            },
            CapacityPool::Emergency,
        ),
    ];

    #[test]
    fn dynamic_limits_for_meta_targets() {
        with_test_data(|tx| {
            for (i, set) in DYNAMIC_LIMITS.iter().enumerate() {
                let pools = for_meta_targets(tx, &set.0, Some(&set.1)).unwrap();
                assert!(pools.into_iter().all(|e| e.cap_pool == set.2), "Case #{i}");
            }
        })
    }

    #[test]
    fn dynamic_limits_for_storage_targets() {
        with_test_data(|tx| {
            for (i, set) in DYNAMIC_LIMITS.iter().enumerate() {
                let pools = for_storage_targets(tx, &set.0, Some(&set.1)).unwrap();
                assert!(
                    pools
                        .into_iter()
                        // Only members of storage pool 1 have a spread in the test data
                        .filter(|e| e.pool_id == 1)
                        .all(|e| e.cap_pool == set.2),
                    "Case #{i}"
                );
            }
        })
    }

    #[test]
    fn dynamic_limits_for_storage_buddy_groups() {
        with_test_data(|tx| {
            for (i, set) in DYNAMIC_LIMITS.iter().enumerate() {
                let pools = for_storage_buddy_groups(tx, &set.0, Some(&set.1)).unwrap();
                assert!(
                    pools
                        .into_iter()
                        // Only members of storage pool 1 have a spread in the test data
                        .filter(|e| e.pool_id == 1)
                        .all(|e| e.cap_pool == set.2),
                    "Case #{i}"
                );
            }
        })
    }

    #[bench]
    fn bench_get_all_cap_pools(b: &mut Bencher) {
        let mut conn = setup_on_disk_db();
        let mut counter = 0;

        b.iter(|| {
            transaction(&mut conn, |tx| {
                for_storage_targets(tx, &FIXED_LIMITS[0].0, None).unwrap();
            });

            transaction(&mut conn, |tx| {
                for_meta_targets(tx, &FIXED_LIMITS[0].0, None).unwrap();
            });

            transaction(&mut conn, |tx| {
                for_storage_buddy_groups(tx, &FIXED_LIMITS[0].0, None).unwrap();
            });

            transaction(&mut conn, |tx| {
                for_meta_buddy_groups(tx, &FIXED_LIMITS[0].0, None).unwrap();
            });

            counter += 1;
        })
    }
}
