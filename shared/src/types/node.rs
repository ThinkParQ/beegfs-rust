use super::*;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::num::{ParseIntError, TryFromIntError};
use std::str::FromStr;

// BeeGFS handles node num ids as u32 in most cases, but there are some messages
// where meta node ID is reused as target ID, and that is u16... So, in reality,
// u32 node IDs don't work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeID(u16);

impl NodeID {
    pub const ZERO: Self = Self(0);
    pub const MGMTD: Self = Self(1);
}

impl Display for NodeID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u16> for NodeID {
    fn from(value: u16) -> Self {
        Self(value)
    }
}

impl From<NodeID> for u16 {
    fn from(value: NodeID) -> u16 {
        value.0
    }
}

impl TryFrom<u32> for NodeID {
    type Error = TryFromIntError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Ok(Self(value.try_into()?))
    }
}

impl From<NodeID> for u32 {
    fn from(value: NodeID) -> u32 {
        value.0 as u32
    }
}

impl_newtype_to_sql!(NodeID => u16);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, BeeSerde)]
pub struct Port(u16);

impl From<Port> for u16 {
    fn from(value: Port) -> Self {
        value.0
    }
}

impl From<u16> for Port {
    fn from(value: u16) -> Self {
        Self(value)
    }
}

impl Display for Port {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Port {
    type Err = ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Port(s.parse()?))
    }
}

impl_newtype_to_sql!(Port => u16);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NodeType {
    #[default]
    Meta,
    Storage,
    Client,
    Management,
}

impl_enum_to_int!(NodeType,
    Meta => 1,
    Storage => 2,
    Client => 3,
    Management => 4
);
impl_enum_to_sql_str!(NodeType,
    Meta => "meta",
    Storage => "storage",
    Client => "client",
    Management => "management"
);
impl_enum_to_user_str!(NodeType,
    Meta => "meta",
    Storage => "storage",
    Client => "client",
    Management => "management"
);

impl From<NodeTypeServer> for NodeType {
    fn from(value: NodeTypeServer) -> Self {
        match value {
            NodeTypeServer::Meta => Self::Meta,
            NodeTypeServer::Storage => Self::Storage,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NodeTypeServer {
    #[default]
    Meta,
    Storage,
}

impl_enum_to_int!(NodeTypeServer,
    Meta => 1,
    Storage => 2
);
impl_enum_to_sql_str!(NodeTypeServer,
    Meta => "meta",
    Storage => "storage"
);
impl_enum_to_user_str!(NodeTypeServer,
    Meta => "meta",
    Storage => "storage"
);
