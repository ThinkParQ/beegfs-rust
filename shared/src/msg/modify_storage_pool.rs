use super::*;

/// Modifies an existing storage pool and adds/removes targets fromto/from this pool
///
/// Targets removed shall be put into the default pool.
///
/// Used by old ctl only
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModifyStoragePool {
    pub pool_id: StoragePoolID,
    pub alias: Option<Vec<u8>>,
    pub add_target_ids: Vec<TargetID>,
    pub remove_target_ids: Vec<TargetID>,
    pub add_buddy_group_ids: Vec<BuddyGroupID>,
    pub remove_buddy_group_ids: Vec<BuddyGroupID>,
}

impl ModifyStoragePool {
    const HAS_DESC: u16 = 1;
    const HAS_ADD_TARGETS: u16 = 2;
    const HAS_REMOVE_TARGETS: u16 = 4;
    const HAS_ADD_GROUPS: u16 = 8;
    const HAS_REMOVE_GROUPS: u16 = 16;
}

impl Msg for ModifyStoragePool {
    const ID: MsgID = 1068;

    fn build_feature_flags(&self) -> u16 {
        let mut flags = 0;
        if self.alias.is_some() {
            flags += Self::HAS_DESC;
        }
        if !self.add_target_ids.is_empty() {
            flags += Self::HAS_ADD_TARGETS;
        }
        if !self.remove_target_ids.is_empty() {
            flags += Self::HAS_REMOVE_TARGETS;
        }
        if !self.add_buddy_group_ids.is_empty() {
            flags += Self::HAS_ADD_GROUPS;
        }
        if !self.remove_buddy_group_ids.is_empty() {
            flags += Self::HAS_REMOVE_GROUPS;
        }

        flags
    }
}

/// Custom BeeSerde impl because actions depend on flags set in the msg header
impl BeeSerde for ModifyStoragePool {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        let flags = ser.msg_feature_flags;

        self.pool_id.serialize(ser)?;

        if flags & Self::HAS_DESC != 0 {
            if let Some(inner) = self.alias.as_ref() {
                ser.cstr(inner.as_ref(), 0)?;
            } else {
                ser.cstr(&[], 0)?;
            }
        }
        if flags & Self::HAS_ADD_TARGETS != 0 {
            ser.seq(self.add_target_ids.iter(), true, |ser, e| e.serialize(ser))?;
        }
        if flags & Self::HAS_REMOVE_TARGETS != 0 {
            ser.seq(self.remove_target_ids.iter(), true, |ser, e| {
                e.serialize(ser)
            })?;
        }
        if flags & Self::HAS_ADD_GROUPS != 0 {
            ser.seq(self.add_buddy_group_ids.iter(), true, |ser, e| {
                e.serialize(ser)
            })?;
        }
        if flags & Self::HAS_REMOVE_GROUPS != 0 {
            ser.seq(self.remove_buddy_group_ids.iter(), true, |ser, e| {
                e.serialize(ser)
            })?;
        }

        Ok(())
    }

    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        let flags = des.msg_feature_flags;

        Ok(Self {
            pool_id: StoragePoolID::deserialize(des)?,
            alias: if flags & Self::HAS_DESC != 0 {
                Some(des.cstr(0)?)
            } else {
                None
            },
            add_target_ids: if flags & Self::HAS_ADD_TARGETS != 0 {
                des.seq(true, |des| TargetID::deserialize(des))?
            } else {
                vec![]
            },
            remove_target_ids: if flags & Self::HAS_REMOVE_TARGETS != 0 {
                des.seq(true, |des| TargetID::deserialize(des))?
            } else {
                vec![]
            },
            add_buddy_group_ids: if flags & Self::HAS_ADD_GROUPS != 0 {
                des.seq(true, |des| BuddyGroupID::deserialize(des))?
            } else {
                vec![]
            },
            remove_buddy_group_ids: if flags & Self::HAS_REMOVE_GROUPS != 0 {
                des.seq(true, |des| BuddyGroupID::deserialize(des))?
            } else {
                vec![]
            },
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BeeSerde)]
pub struct ModifyStoragePoolResp {
    pub result: OpsErr,
}

impl Msg for ModifyStoragePoolResp {
    const ID: MsgID = 1069;
}
