use super::*;
use serde::{Deserialize, Serialize};
use std::fmt::Display;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct StoragePoolID(u16);

impl StoragePoolID {
    pub const ZERO: Self = Self(0);
    pub const DEFAULT: Self = Self(1);
}

impl From<StoragePoolID> for u16 {
    fn from(value: StoragePoolID) -> Self {
        value.0
    }
}

impl From<u16> for StoragePoolID {
    fn from(value: u16) -> Self {
        Self(value)
    }
}

impl Display for StoragePoolID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl_newtype_to_sql!(StoragePoolID => u16);

impl TryFrom<Option<StoragePoolID>> for StoragePoolID {
    type Error = anyhow::Error;

    fn try_from(value: Option<StoragePoolID>) -> Result<Self, Self::Error> {
        value.ok_or_else(|| anyhow::anyhow!("Expected Some(StoragePoolID) but it is None"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CapacityPool {
    Normal,
    Low,
    Emergency,
}

impl CapacityPool {
    pub fn lowest(cap_pool_1: Self, cap_pool_2: Self) -> Self {
        std::cmp::max(cap_pool_1, cap_pool_2)
    }
}

impl_enum_to_int!(CapacityPool, Normal => 0, Low => 1, Emergency => 2);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapPoolLimits {
    pub inodes_low: Inodes,
    pub inodes_emergency: Inodes,
    pub space_low: Space,
    pub space_emergency: Space,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CapPoolDynamicLimits {
    pub inodes_normal_threshold: Inodes,
    pub inodes_low_threshold: Inodes,
    pub space_normal_threshold: Space,
    pub space_low_threshold: Space,
    pub inodes_low: Inodes,
    pub inodes_emergency: Inodes,
    pub space_low: Space,
    pub space_emergency: Space,
}
