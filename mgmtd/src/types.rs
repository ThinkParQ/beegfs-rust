use serde::{Deserialize, Serialize};
use shared::parser::integer_with_generic_unit;

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
