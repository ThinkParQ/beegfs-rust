use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum QuotaIDType {
    #[default]
    User,
    Group,
}

impl_enum_to_int!(QuotaIDType,
    User => 1,
    Group => 2
);
impl_enum_to_sql_str!(QuotaIDType,
    User => "user",
    Group => "group"
);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum QuotaType {
    #[default]
    Space,
    Inodes,
}

impl_enum_to_int!(QuotaType,
    Space => 1,
    Inodes => 2
);

impl_enum_to_sql_str!(QuotaType,
    Space => "space",
    Inodes => "inodes"
);

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    BeeSerde,
)]
pub struct QuotaID(u32);

impl QuotaID {
    pub const ZERO: Self = Self(0);
}

impl From<u32> for QuotaID {
    fn from(id: u32) -> Self {
        Self(id)
    }
}

impl From<QuotaID> for u32 {
    fn from(id: QuotaID) -> Self {
        id.0
    }
}

impl AsRef<u32> for QuotaID {
    fn as_ref(&self) -> &u32 {
        &self.0
    }
}

impl_newtype_to_sql!(QuotaID => u32);

impl Display for QuotaID {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}
