//! BeeGFS network message definitions

use crate::bee_serde::{BeeSerde, *};
use crate::types::*;
use anyhow::Result;
use bee_serde_derive::BeeSerde;
use std::collections::{HashMap, HashSet};

pub mod buddy_group;
pub mod header;
pub mod misc;
pub mod node;
pub mod quota;
pub mod storage_pool;
pub mod target;

/// The BeeGFS message ID as defined in `NetMsgTypes.h`
pub type MsgID = u16;

/// A BeeGFS message
///
/// A struct that implements `Msg` represents a BeeGFS message that is compatible with other C/C++
/// based BeeGFS components.
pub trait Msg: BeeSerde + std::fmt::Debug + Clone + Send + Sync + 'static {
    /// Message type as defined in NetMessageTypes.h
    const ID: MsgID;

    /// Returns the feature flags
    ///
    /// Feature flags are a u16 field in the message header and are sometimes used to control
    /// (de-)serialization. This function provides them to the serializer.
    fn build_feature_flags(&self) -> u16 {
        0
    }
}

/// Matches the `FhgfsOpsErr` value from the BeeGFS C/C++ codebase.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, BeeSerde)]
pub struct OpsErr(i32);

impl OpsErr {
    pub const SUCCESS: Self = Self(0);
    pub const INTERNAL: Self = Self(1);
    pub const UNKNOWN_NODE: Self = Self(5);
    pub const EXISTS: Self = Self(7);
    pub const NOTEMPTY: Self = Self(13);
    pub const UNKNOWN_TARGET: Self = Self(15);
    pub const INVAL: Self = Self(20);
    pub const AGAIN: Self = Self(22);
    pub const UNKNOWN_POOL: Self = Self(30);
}
