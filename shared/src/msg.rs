//! BeeGFS network message definitions

use crate::bee_serde;
use crate::bee_serde::*;
use crate::types::*;
use anyhow::Result;
use derive_bee_serde::BeeSerde;
use std::collections::{HashMap, HashSet};

pub mod types;
use types::*;

mod definitions;
pub use definitions::*;

mod header;
pub(crate) use header::Header;

pub trait Msg: BeeSerde + std::fmt::Debug + Clone + Send + Sync + 'static {
    /// Message type as defined in NetMessageTypes.h
    const ID: MsgID;

    /// returns the feature flags set built from data
    fn build_feature_flags(&self) -> u16 {
        0
    }
}
