use super::*;
use crate::draw::log_draw_error;
use nvim_oxi::Result;
use nvim_oxi::api;
use nvim_oxi::api::opts::{OptionOpts, OptionScope};
use nvim_oxi::api::types::{WindowConfig, WindowRelativeTo, WindowStyle};
use nvim_oxi_utils::handles;
use std::collections::{BinaryHeap, HashMap, HashSet};

include!("ops/adaptive.rs");
include!("ops/windows.rs");
include!("ops/acquire.rs");
include!("ops/cleanup.rs");
include!("ops/snapshot.rs");
#[cfg(test)]
include!("ops/tests.rs");
