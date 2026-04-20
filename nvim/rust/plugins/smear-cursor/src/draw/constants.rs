//! Shared draw constants consumed across render, cleanup, and observation paths.

pub(crate) const EXTMARK_ID: u32 = 999;
pub(super) const PREPAINT_EXTMARK_ID: u32 = 1001;
pub(crate) const PREPAINT_BUFFER_TYPE: &str = "nofile";
pub(crate) const PREPAINT_BUFFER_FILETYPE: &str = "smear-cursor-prepaint";
pub(super) const PREPAINT_HIGHLIGHT_GROUP: &str = "Cursor";
pub(crate) const BRAILLE_CODE_MIN: i64 = 0x2800;
pub(crate) const BRAILLE_CODE_MAX: i64 = 0x28FF;
pub(crate) const OCTANT_CODE_MIN: i64 = 0x1CD00;
pub(crate) const OCTANT_CODE_MAX: i64 = 0x1CDE7;
pub(crate) const PARTICLE_ZINDEX_OFFSET: u32 = 1;
