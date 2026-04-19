//! Shared position vocabulary for `smear-cursor`.
//!
//! This module owns the crate's retained position primitives so discrete editor
//! cells, viewport bounds, cursor observations, and continuous render-space
//! geometry stay distinct behind validated constructors.

mod observation;
mod render;
mod surface;
#[cfg(test)]
mod tests;
mod validated;

pub(crate) use observation::CursorObservation;
pub(crate) use observation::ObservedCell;
pub(crate) use render::RenderPoint;
pub(crate) use render::corners_center;
pub(crate) use render::current_visual_cursor_anchor;
pub(crate) use render::display_metric_row_scale;
pub(crate) use surface::WindowSurfaceSnapshot;
pub(crate) use validated::BufferLine;
pub(crate) use validated::ScreenCell;
pub(crate) use validated::SurfaceId;
pub(crate) use validated::ViewportBounds;
