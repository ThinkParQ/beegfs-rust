//! Functions for calculating targets and buddy groups capacity pools.
//!
//! Pools are calculated based on the behavior in old management.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use shared::parser::integer_with_generic_unit;
use shared::types::CapacityPool;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapPoolLimits {
    #[serde(with = "integer_with_generic_unit")]
    pub inodes_low: u64,
    #[serde(with = "integer_with_generic_unit")]
    pub inodes_emergency: u64,
    #[serde(with = "integer_with_generic_unit")]
    pub space_low: u64,
    #[serde(with = "integer_with_generic_unit")]
    pub space_emergency: u64,
}

impl CapPoolLimits {
    pub fn check(&self) -> anyhow::Result<()> {
        if self.space_low < self.space_emergency || self.inodes_low < self.inodes_emergency {
            bail!("The low limit is lower than the emergency limit");
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CapPoolDynamicLimits {
    #[serde(with = "integer_with_generic_unit")]
    pub inodes_normal_threshold: u64,
    #[serde(with = "integer_with_generic_unit")]
    pub inodes_low_threshold: u64,
    #[serde(with = "integer_with_generic_unit")]
    pub space_normal_threshold: u64,
    #[serde(with = "integer_with_generic_unit")]
    pub space_low_threshold: u64,
    #[serde(with = "integer_with_generic_unit")]
    pub inodes_low: u64,
    #[serde(with = "integer_with_generic_unit")]
    pub inodes_emergency: u64,
    #[serde(with = "integer_with_generic_unit")]
    pub space_low: u64,
    #[serde(with = "integer_with_generic_unit")]
    pub space_emergency: u64,
}

impl CapPoolDynamicLimits {
    pub fn check(&self) -> anyhow::Result<()> {
        if self.space_low < self.space_emergency || self.inodes_low < self.inodes_emergency {
            bail!("the low limit is lower than the emergency limit");
        }

        Ok(())
    }
}

pub(crate) trait CapacityInfo {
    fn free_space(&self) -> u64;
    fn free_inodes(&self) -> u64;
}

#[derive(Debug)]
pub(crate) struct CapPoolCalculator {
    limits: CapPoolLimits,
}

impl CapPoolCalculator {
    pub(crate) fn new(
        limits: CapPoolLimits,
        dynamic_limits: Option<&CapPoolDynamicLimits>,
        values: impl IntoIterator<Item = impl CapacityInfo>,
    ) -> Result<Self> {
        if let Some(dl) = dynamic_limits {
            Self::new_dynamic(limits, dl, values)
        } else {
            Self::new_static(limits)
        }
    }

    pub(crate) fn new_static(limits: CapPoolLimits) -> Result<Self> {
        limits.check().context("cap pool calculator")?;

        Ok(Self { limits })
    }

    pub(crate) fn new_dynamic(
        mut limits: CapPoolLimits,
        dynamic_limits: &CapPoolDynamicLimits,
        values: impl IntoIterator<Item = impl CapacityInfo>,
    ) -> Result<Self> {
        limits.check().context("cap pool calculator")?;
        dynamic_limits.check().context("cap pool calculator")?;

        let mut normal_space = MinMax::default();
        let mut normal_inodes = MinMax::default();
        let mut low_space = MinMax::default();
        let mut low_inodes = MinMax::default();

        for e in values.into_iter() {
            if e.free_space() >= limits.space_low && e.free_inodes() >= limits.inodes_low {
                normal_space.apply(e.free_space());
                normal_inodes.apply(e.free_inodes());
            } else if e.free_space() >= limits.space_emergency
                && e.free_inodes() >= limits.inodes_emergency
            {
                low_space.apply(e.free_space());
                low_inodes.apply(e.free_inodes());
            }
        }

        if normal_space.spread() > dynamic_limits.space_normal_threshold {
            limits.space_low = dynamic_limits.space_low;
        }
        if normal_inodes.spread() > dynamic_limits.inodes_normal_threshold {
            limits.inodes_low = dynamic_limits.inodes_low;
        }
        if low_space.spread() > dynamic_limits.space_low_threshold {
            limits.space_emergency = dynamic_limits.space_emergency;
        }
        if low_inodes.spread() > dynamic_limits.inodes_low_threshold {
            limits.inodes_emergency = dynamic_limits.inodes_emergency;
        }

        Ok(Self { limits })
    }

    pub(crate) fn cap_pool(&self, space: u64, inodes: u64) -> CapacityPool {
        if space >= self.limits.space_low && inodes >= self.limits.inodes_low {
            CapacityPool::Normal
        } else if space >= self.limits.space_emergency && inodes >= self.limits.inodes_emergency {
            CapacityPool::Low
        } else {
            CapacityPool::Emergency
        }
    }
}

#[derive(Default)]
struct MinMax {
    min: u64,
    max: u64,
}

impl MinMax {
    fn apply(&mut self, v: u64) {
        if self.min == 0 && self.max == 0 {
            self.min = v;
            self.max = v;
        } else if v < self.min {
            self.min = v;
        } else if v > self.max {
            self.max = v;
        }
    }

    fn spread(&self) -> u64 {
        self.max - self.min
    }
}

#[cfg(test)]
mod test {
    use super::*;

    impl CapacityInfo for &(u64, u64) {
        fn free_space(&self) -> u64 {
            self.0
        }

        fn free_inodes(&self) -> u64 {
            self.1
        }
    }

    fn limits() -> CapPoolLimits {
        CapPoolLimits {
            inodes_low: 70,
            inodes_emergency: 30,
            space_low: 70,
            space_emergency: 30,
        }
    }

    fn dynamic_limits() -> &'static CapPoolDynamicLimits {
        &CapPoolDynamicLimits {
            inodes_normal_threshold: 10,
            inodes_low_threshold: 10,
            space_normal_threshold: 10,
            space_low_threshold: 10,
            inodes_low: 170,
            inodes_emergency: 130,
            space_low: 170,
            space_emergency: 130,
        }
    }

    #[test]
    fn static_limits() {
        let c = CapPoolCalculator::new_static(limits()).unwrap();

        assert_eq!(CapacityPool::Normal, c.cap_pool(100, 100));
        assert_eq!(CapacityPool::Low, c.cap_pool(50, 50));
        assert_eq!(CapacityPool::Low, c.cap_pool(50, 100));
        assert_eq!(CapacityPool::Low, c.cap_pool(100, 50));
        assert_eq!(CapacityPool::Emergency, c.cap_pool(10, 10));
        assert_eq!(CapacityPool::Emergency, c.cap_pool(10, 100));
        assert_eq!(CapacityPool::Emergency, c.cap_pool(100, 10));
    }

    #[test]
    fn no_spread() {
        let c =
            CapPoolCalculator::new_dynamic(limits(), dynamic_limits(), &[(100, 100), (100, 100)])
                .unwrap();

        assert_eq!(CapacityPool::Normal, c.cap_pool(100, 100));
        assert_eq!(CapacityPool::Low, c.cap_pool(50, 50));
        assert_eq!(CapacityPool::Low, c.cap_pool(50, 100));
        assert_eq!(CapacityPool::Low, c.cap_pool(100, 50));
        assert_eq!(CapacityPool::Emergency, c.cap_pool(10, 10));
        assert_eq!(CapacityPool::Emergency, c.cap_pool(10, 100));
        assert_eq!(CapacityPool::Emergency, c.cap_pool(100, 10));
    }

    #[test]
    fn space_spread() {
        let normal_only = CapPoolCalculator::new_dynamic(
            limits(),
            dynamic_limits(),
            &[(40, 100), (50, 100), (80, 100), (91, 100)],
        )
        .unwrap();

        assert_eq!(CapacityPool::Normal, normal_only.cap_pool(170, 100));
        assert_eq!(CapacityPool::Low, normal_only.cap_pool(169, 100));
        assert_eq!(CapacityPool::Low, normal_only.cap_pool(30, 100));
        assert_eq!(CapacityPool::Emergency, normal_only.cap_pool(29, 100));

        let both = CapPoolCalculator::new_dynamic(
            limits(),
            dynamic_limits(),
            &[(30, 100), (41, 100), (70, 100), (81, 100)],
        )
        .unwrap();

        assert_eq!(CapacityPool::Normal, both.cap_pool(170, 100));
        assert_eq!(CapacityPool::Low, both.cap_pool(169, 100));
        assert_eq!(CapacityPool::Low, both.cap_pool(130, 100));
        assert_eq!(CapacityPool::Emergency, both.cap_pool(129, 100));
    }

    #[test]
    fn inode_spread() {
        let normal_only = CapPoolCalculator::new_dynamic(
            limits(),
            dynamic_limits(),
            &[(100, 40), (100, 50), (100, 80), (100, 91)],
        )
        .unwrap();

        assert_eq!(CapacityPool::Normal, normal_only.cap_pool(100, 170));
        assert_eq!(CapacityPool::Low, normal_only.cap_pool(100, 169));
        assert_eq!(CapacityPool::Low, normal_only.cap_pool(100, 30));
        assert_eq!(CapacityPool::Emergency, normal_only.cap_pool(100, 29));

        let both = CapPoolCalculator::new_dynamic(
            limits(),
            dynamic_limits(),
            &[(100, 40), (100, 51), (100, 80), (100, 91)],
        )
        .unwrap();

        assert_eq!(CapacityPool::Normal, both.cap_pool(100, 170));
        assert_eq!(CapacityPool::Low, both.cap_pool(100, 169));
        assert_eq!(CapacityPool::Low, both.cap_pool(100, 130));
        assert_eq!(CapacityPool::Emergency, both.cap_pool(100, 129));
    }

    #[test]
    fn limit_validity() {
        CapPoolCalculator::new_static(CapPoolLimits {
            inodes_low: 0,
            inodes_emergency: 0,
            space_low: 0,
            space_emergency: 0,
        })
        .unwrap();

        CapPoolCalculator::new_static(CapPoolLimits {
            inodes_low: 100,
            inodes_emergency: 100,
            space_low: 100,
            space_emergency: 100,
        })
        .unwrap();

        CapPoolCalculator::new_static(CapPoolLimits {
            inodes_low: 100,
            inodes_emergency: 200,
            space_low: 100,
            space_emergency: 100,
        })
        .unwrap_err();

        CapPoolCalculator::new_static(CapPoolLimits {
            inodes_low: 100,
            inodes_emergency: 100,
            space_low: 100,
            space_emergency: 200,
        })
        .unwrap_err();

        CapPoolCalculator::new_dynamic(
            CapPoolLimits {
                inodes_low: 0,
                inodes_emergency: 0,
                space_low: 0,
                space_emergency: 0,
            },
            &CapPoolDynamicLimits {
                inodes_normal_threshold: 0,
                inodes_low_threshold: 0,
                space_normal_threshold: 0,
                space_low_threshold: 0,
                inodes_low: 100,
                inodes_emergency: 200,
                space_low: 0,
                space_emergency: 0,
            },
            &[(0, 0)],
        )
        .unwrap_err();

        CapPoolCalculator::new_dynamic(
            CapPoolLimits {
                inodes_low: 0,
                inodes_emergency: 0,
                space_low: 0,
                space_emergency: 0,
            },
            &CapPoolDynamicLimits {
                inodes_normal_threshold: 0,
                inodes_low_threshold: 0,
                space_normal_threshold: 0,
                space_low_threshold: 0,
                inodes_low: 0,
                inodes_emergency: 0,
                space_low: 100,
                space_emergency: 200,
            },
            &[(0, 0)],
        )
        .unwrap_err();
    }
}
