use super::*;
use crate::draw::log_draw_error;
use nvim_oxi::Result;
use nvim_oxi::api;
use nvim_oxi::api::opts::OptionOpts;
use nvim_oxi::api::opts::OptionScope;
use nvim_oxi::api::types::WindowConfig;
use nvim_oxi::api::types::WindowRelativeTo;
use nvim_oxi::api::types::WindowStyle;
use nvimrs_nvim_oxi_utils::handles;
use std::collections::BinaryHeap;
#[cfg(test)]
use std::collections::HashMap;
use std::collections::HashSet;

include!("ops/adaptive.rs");
include!("ops/windows.rs");
include!("ops/acquire.rs");
include!("ops/cleanup.rs");
include!("ops/snapshot.rs");
#[cfg(test)]
include!("ops/tests.rs");
