use super::*;
use std::fmt::Display;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct BuddyGroupID(u16);

impl BuddyGroupID {
    pub const ZERO: BuddyGroupID = BuddyGroupID(0);
}

impl From<u16> for BuddyGroupID {
    fn from(id: u16) -> Self {
        Self(id)
    }
}

impl From<BuddyGroupID> for u16 {
    fn from(id: BuddyGroupID) -> u16 {
        id.0
    }
}

impl From<BuddyGroupID> for u32 {
    fn from(value: BuddyGroupID) -> u32 {
        value.0 as u32
    }
}

impl Display for BuddyGroupID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl_newtype_to_sql!(BuddyGroupID => u16);
