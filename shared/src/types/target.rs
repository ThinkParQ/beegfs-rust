use super::*;
use std::fmt::{Debug, Display};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, BeeSerde)]
pub struct TargetID(u16);

impl TargetID {
    pub const ZERO: Self = Self(0);
}

impl Display for TargetID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u16> for TargetID {
    fn from(value: u16) -> Self {
        Self(value)
    }
}

impl From<TargetID> for u16 {
    fn from(value: TargetID) -> u16 {
        value.0
    }
}

impl_newtype_to_sql!(TargetID => u16);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TargetConsistencyState {
    #[default]
    Good,
    NeedsResync,
    Bad,
}

impl_enum_to_int!(TargetConsistencyState,
    Good => 0,
    NeedsResync => 1,
    Bad => 2
);

impl BeeSerde for TargetConsistencyState {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.u8((*self).into())
    }

    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        des.u8()?.try_into()
    }
}

impl_enum_to_sql_str!(TargetConsistencyState,
    Good => "good",
    NeedsResync => "needs_resync",
    Bad => "bad"
);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TargetReachabilityState {
    #[default]
    Online,
    ProbablyOffline,
    Offline,
}

impl_enum_to_int!(TargetReachabilityState,
    Online => 0,
    ProbablyOffline => 1,
    Offline => 2
);

impl BeeSerde for TargetReachabilityState {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.u8((*self).into())
    }

    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        des.u8()?.try_into()
    }
}
