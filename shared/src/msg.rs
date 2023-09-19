//! BeeGFS network message definitions

use crate::bee_serde::*;
use crate::types::*;
use anyhow::Result;
use bee_serde_derive::BeeSerde;
use std::collections::{HashMap, HashSet};

pub mod types;
use types::*;

mod definitions;
pub use definitions::*;

pub mod header;

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
