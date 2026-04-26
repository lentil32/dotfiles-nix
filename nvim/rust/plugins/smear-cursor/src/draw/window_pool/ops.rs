use super::*;
use crate::draw::TrackedResourceCloseOutcome;
use crate::draw::TrackedResourceCloseSummary;
use crate::draw::floating_windows::EventIgnoreGuard;
use crate::draw::log_draw_error;
use crate::host::BufferHandle;
use crate::host::DrawResourcePort;
use crate::host::FloatingWindowEnter;
use crate::host::NamespaceId;
use crate::host::NeovimHost;
use crate::host::TabHandle;
use crate::host::api;
use crate::host::api::opts::OptionScope;
use crate::host::api::types::WindowConfig;
use crate::host::api::types::WindowRelativeTo;
use crate::host::api::types::WindowStyle;
use nvim_oxi::Result;
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
