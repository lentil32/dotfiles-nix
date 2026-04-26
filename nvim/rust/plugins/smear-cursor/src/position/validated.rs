//! Validated discrete position primitives for screen, buffer, and surface facts.

use std::num::NonZeroI64;

use crate::host::BufferHandle;

use super::render::RenderPoint;

pub(super) fn positive_i64(value: i64) -> Option<NonZeroI64> {
    let value = NonZeroI64::new(value)?;
    (value.get() > 0).then_some(value)
}

/// A one-based row/column cell in editor screen space.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct ScreenCell {
    row: NonZeroI64,
    col: NonZeroI64,
}

impl ScreenCell {
    pub(crate) fn new(row: i64, col: i64) -> Option<Self> {
        let (Some(row), Some(col)) = (positive_i64(row), positive_i64(col)) else {
            return None;
        };
        Some(Self { row, col })
    }

    /// Validates an exact host-reported screen cell without rounding.
    #[cfg(test)]
    pub(crate) fn from_host_point(point: RenderPoint) -> Option<Self> {
        if !point.row.is_finite()
            || !point.col.is_finite()
            || point.row.fract() != 0.0
            || point.col.fract() != 0.0
            || point.row < 1.0
            || point.col < 1.0
            || point.row > i64::MAX as f64
            || point.col > i64::MAX as f64
        {
            return None;
        }

        Self::new(point.row as i64, point.col as i64)
    }

    pub(crate) fn from_rounded_point(point: RenderPoint) -> Option<Self> {
        if !point.row.is_finite() || !point.col.is_finite() {
            return None;
        }

        let rounded_row = point.row.round();
        let rounded_col = point.col.round();
        if rounded_row < 1.0
            || rounded_col < 1.0
            || rounded_row > i64::MAX as f64
            || rounded_col > i64::MAX as f64
        {
            return None;
        }

        Self::new(rounded_row as i64, rounded_col as i64)
    }

    pub(crate) const fn row(self) -> i64 {
        self.row.get()
    }

    pub(crate) const fn col(self) -> i64 {
        self.col.get()
    }
}

/// A one-based buffer line captured from the host.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct BufferLine(NonZeroI64);

impl BufferLine {
    pub(crate) fn new(line: i64) -> Option<Self> {
        Some(Self(positive_i64(line)?))
    }

    pub(crate) const fn value(self) -> i64 {
        self.0.get()
    }
}

/// Inclusive viewport bounds expressed in one-based editor cells.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct ViewportBounds {
    max_row: NonZeroI64,
    max_col: NonZeroI64,
}

impl ViewportBounds {
    pub(crate) fn new(max_row: i64, max_col: i64) -> Option<Self> {
        Some(Self {
            max_row: positive_i64(max_row)?,
            max_col: positive_i64(max_col)?,
        })
    }

    pub(crate) const fn max_row(self) -> i64 {
        self.max_row.get()
    }

    pub(crate) const fn max_col(self) -> i64 {
        self.max_col.get()
    }
}

/// The live window and buffer identity for an observed surface.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct SurfaceId {
    window_handle: NonZeroI64,
    buffer_handle: BufferHandle,
}

impl SurfaceId {
    pub(crate) fn new(window_handle: i64, buffer_handle: i64) -> Option<Self> {
        Some(Self {
            window_handle: positive_i64(window_handle)?,
            buffer_handle: BufferHandle::new(buffer_handle)?,
        })
    }

    pub(crate) const fn window_handle(self) -> i64 {
        self.window_handle.get()
    }

    pub(crate) const fn buffer_handle(self) -> BufferHandle {
        self.buffer_handle
    }
}
