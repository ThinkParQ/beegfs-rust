use crate::bee_serde::{self, *};
use crate::impl_enum_to_int;
use crate::types::*;
use anyhow::Result;
use derive_bee_serde::BeeSerde;
use std::collections::HashMap;

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct Node {
    #[bee_serde(as = CStr<0>)]
    pub alias: EntityAlias,
    #[bee_serde(as = Seq<false, _>)]
    pub nic_list: Vec<Nic>,
    #[bee_serde(as = Int<u32>)]
    pub num_id: NodeID,
    pub port: Port,
    pub _unused_tcp_port: Port,
    #[bee_serde(as = Int<u8>)]
    pub node_type: NodeType,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct CombinedTargetState {
    pub reachability: TargetReachabilityState,
    pub consistency: TargetConsistencyState,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct TargetInfo {
    pub target_id: TargetID,
    #[bee_serde(as = CStr<4>)]
    pub path: Vec<u8>,
    #[bee_serde(as = Int<i64>)]
    pub total_space: u64,
    #[bee_serde(as = Int<i64>)]
    pub free_space: u64,
    #[bee_serde(as = Int<i64>)]
    pub total_inodes: u64,
    #[bee_serde(as = Int<i64>)]
    pub free_inodes: u64,
    pub consistency_state: TargetConsistencyState,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct StoragePool {
    pub id: StoragePoolID,
    #[bee_serde(as = CStr<0>)]
    pub alias: EntityAlias,
    #[bee_serde(as = Seq<true, _>)]
    pub targets: Vec<TargetID>,
    #[bee_serde(as = Seq<true, _>)]
    pub buddy_groups: Vec<BuddyGroupID>,
    pub target_cap_pools: TargetCapacityPools,
    pub buddy_cap_pools: BuddyGroupCapacityPools,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TargetCapacityPools {
    pub pools: Vec<Vec<TargetID>>,
    pub grouped_target_pools: Vec<HashMap<NodeID, Vec<TargetID>>>,
    pub target_map: HashMap<TargetID, NodeID>,
}

impl BeeSerde for TargetCapacityPools {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.seq(self.pools.iter(), true, |ser, e| {
            ser.seq(e.iter(), true, |ser, e| e.serialize(ser))
        })?;
        ser.seq(self.grouped_target_pools.iter(), true, |ser, e| {
            ser.map(
                e.iter(),
                false,
                |ser, k| ser.u32((*k).into()),
                |ser, v| ser.seq(v.iter(), true, |ser, e| e.serialize(ser)),
            )
        })?;
        ser.map(
            self.target_map.iter(),
            false,
            |ser, k| k.serialize(ser),
            |ser, v| ser.u32((*v).into()),
        )?;
        Ok(())
    }

    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        Ok(Self {
            pools: des.seq(true, |des| des.seq(true, |des| TargetID::deserialize(des)))?,
            grouped_target_pools: des.seq(true, |des| {
                des.map(
                    false,
                    |des| Ok(des.u32()?.try_into()?),
                    |des| des.seq(true, |des| TargetID::deserialize(des)),
                )
            })?,
            target_map: des.map(
                false,
                |des| TargetID::deserialize(des),
                |des| Ok(des.u32()?.try_into()?),
            )?,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct BuddyGroup {
    pub primary_target_id: TargetID,
    pub secondary_target_id: TargetID,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StorageBuddyGroup {
    pub primary_target_id: TargetID,
    pub secondary_target_id: TargetID,
    pub pool_id: StoragePoolID,
    pub cap_pool: CapacityPool,
}

impl From<StorageBuddyGroup> for BuddyGroup {
    fn from(value: StorageBuddyGroup) -> Self {
        Self {
            primary_target_id: value.primary_target_id,
            secondary_target_id: value.secondary_target_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MetaBuddyGroup {
    pub first_target: TargetID,
    pub second_target: TargetID,
    pub cap_pool: CapacityPool,
}

impl From<MetaBuddyGroup> for BuddyGroup {
    fn from(value: MetaBuddyGroup) -> Self {
        Self {
            primary_target_id: value.first_target,
            secondary_target_id: value.second_target,
        }
    }
}

/// BeeGFS actually deserializes this into a NodeCapacityPools object
/// which contains TargetNumIDs but in reality are buddy group ids.
/// Since TargetNumIDs are represented as u16, it's very important
/// to only change them together if this is ever changed!
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct BuddyGroupCapacityPools {
    pub pools: Vec<Vec<BuddyGroupID>>,
}

impl BeeSerde for BuddyGroupCapacityPools {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.seq(self.pools.iter(), true, |ser, e| {
            ser.seq(e.iter(), true, |ser, e| e.serialize(ser))
        })
    }

    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        Ok(Self {
            pools: des.seq(true, |des| {
                des.seq(true, |des| BuddyGroupID::deserialize(des))
            })?,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CapacityPoolQueryType {
    #[default]
    Meta,
    Storage,
    MetaMirrored,
    StorageMirrored,
}

impl_enum_to_int!(CapacityPoolQueryType, Meta => 0, Storage => 1, MetaMirrored => 2, StorageMirrored => 3);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GetQuotaInfoTransferMethod {
    #[default]
    AllTargetsOneRequest = 0,
    AllTargetsOneRequestPerTarget = 1,
    SingleTarget = 2,
}

impl_enum_to_int!(GetQuotaInfoTransferMethod,
    AllTargetsOneRequest => 0,
    AllTargetsOneRequestPerTarget => 1,
    SingleTarget => 2
);

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct QuotaEntry {
    pub space: u64,
    pub inodes: u64,
    pub id: QuotaID,
    #[bee_serde(as = Int<i32>)]
    pub id_type: QuotaIDType,
    #[bee_serde(as = BoolAsInt<u8>)]
    pub valid: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct QuotaDefaultLimits {
    pub user_inode_limit: u64,
    pub user_space_limit: u64,
    pub group_inode_limit: u64,
    pub group_space_limit: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum QuotaInodeSupport {
    #[default]
    Unknown,
    AllBlockDevices,
    SomeBlockDevices,
    NoBlockDevices,
}

impl_enum_to_int!(QuotaInodeSupport,
    Unknown => 0,
    AllBlockDevices => 1,
    SomeBlockDevices => 2,
    NoBlockDevices => 3
);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum QuotaQueryType {
    #[default]
    None,
    Single,
    Range,
    List,
    All,
}

impl_enum_to_int!(QuotaQueryType,
    None => 0,
    Single => 1,
    Range => 2,
    List => 3,
    All => 4
);
