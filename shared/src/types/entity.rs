use super::*;
use anyhow::{bail, Result};
use core::hash::Hash;
#[cfg(feature = "protobuf")]
use protobuf::beegfs as pb;
use regex::Regex;
use std::fmt::{Debug, Display};
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntityType {
    Node,
    Target,
    BuddyGroup,
    Pool,
}

impl_enum_user_str! {EntityType,
    EntityType::Node => "node",
    EntityType::Target => "target",
    EntityType::Pool => "pool",
    EntityType::BuddyGroup => "buddy group"
}

#[cfg(feature = "protobuf")]
impl_enum_protobuf_traits! {EntityType => pb::EntityType,
    unspecified => pb::EntityType::Unspecified,
    EntityType::Node => pb::EntityType::Node,
    EntityType::Target => pb::EntityType::Target,
    EntityType::Pool => pb::EntityType::Pool,
    EntityType::BuddyGroup => pb::EntityType::BuddyGroup,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Alias(String);

static REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9-_.]+$").expect("Regex must be valid"));

impl TryFrom<String> for Alias {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        // Max length allowed is 32 bytes (which is equal to 32 characters with the allowed set). If
        // the length limit is ever changed, it should be reflected on the client which uses fixed
        // size buffers to store the alias.
        if value.len() > 32 {
            bail!("invalid alias '{value}': max length is 32 characters");
        }

        if !REGEX.is_match(&value) {
            bail!("invalid alias '{value}': must start with a letter and may only contain letters, digits, '-', '_' and '.'");
        }

        Ok(Self(value))
    }
}

impl TryFrom<&str> for Alias {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_from(value.to_owned())
    }
}

impl AsRef<str> for Alias {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl From<Alias> for String {
    fn from(value: Alias) -> Self {
        value.0
    }
}

impl Display for Alias {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LegacyId {
    pub node_type: NodeType,
    pub num_id: u32,
}

impl Display for LegacyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.node_type, self.num_id)
    }
}

#[cfg(feature = "protobuf")]
impl TryFrom<pb::LegacyId> for LegacyId {
    type Error = anyhow::Error;

    fn try_from(value: pb::LegacyId) -> Result<Self, Self::Error> {
        let node_type = value.node_type().try_into()?;

        if value.num_id == 0 {
            bail!("num_id cannot be 0: {value:?}");
        }

        Ok(Self {
            node_type,
            num_id: value.num_id,
        })
    }
}

#[cfg(feature = "protobuf")]
impl From<LegacyId> for pb::LegacyId {
    fn from(value: LegacyId) -> Self {
        Self {
            num_id: value.num_id,
            node_type: pb::NodeType::from(value.node_type).into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EntityId {
    Alias(Alias),
    LegacyID(LegacyId),
    Uid(Uid),
}

impl Display for EntityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EntityId::Alias(ref alias) => Display::fmt(alias, f),
            EntityId::LegacyID(ref legacy_id) => Display::fmt(legacy_id, f),
            EntityId::Uid(uid) => write!(f, "uid:{uid}"),
        }
    }
}

#[cfg(feature = "protobuf")]
impl TryFrom<pb::EntityIdSet> for EntityId {
    type Error = anyhow::Error;

    fn try_from(value: pb::EntityIdSet) -> Result<Self, Self::Error> {
        let variant = if let Some(uid) = value.uid {
            Self::Uid(uid)
        } else if let Some(alias) = value.alias {
            Self::Alias(alias.try_into()?)
        } else if let Some(id) = value.legacy_id {
            Self::LegacyID(id.try_into()?)
        } else {
            bail!("input contains no info: {value:?}");
        };

        Ok(variant)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EntityIdSet {
    pub uid: Uid,
    pub alias: Alias,
    pub legacy_id: LegacyId,
}

impl EntityIdSet {
    pub fn node_type(&self) -> NodeType {
        self.legacy_id.node_type
    }

    pub fn num_id(&self) -> u32 {
        self.legacy_id.num_id
    }
}

impl Display for EntityIdSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}[{}, uid:{}]", self.alias, self.legacy_id, self.uid)
    }
}

#[cfg(feature = "protobuf")]
impl From<EntityIdSet> for pb::EntityIdSet {
    fn from(value: EntityIdSet) -> Self {
        Self {
            uid: Some(value.uid),
            alias: Some(value.alias.into()),
            legacy_id: Some(value.legacy_id.into()),
        }
    }
}
