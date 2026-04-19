//! Observation-time cursor facts built on the shared position vocabulary.
//!
//! Any [`ScreenCell`] that crosses this boundary is already projected into display
//! space. Event-layer readers may retain raw host probes for diagnostics, trace
//! labels, or cache seeding, but reducer-owned observation state never stores raw
//! buffer-column cursor coordinates.

use super::validated::BufferLine;
use super::validated::ScreenCell;

/// The normalized exactness of an observed display-space cursor cell.
///
/// [`ObservedCell::Exact`] and [`ObservedCell::Deferred`] both carry projected
/// display-space [`ScreenCell`] values. `Deferred` means the reader still owes a
/// fresher exact pass; it does not mean the cell is still in raw host
/// coordinates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ObservedCell {
    Unavailable,
    Exact(ScreenCell),
    Deferred(ScreenCell),
}

impl ObservedCell {
    pub(crate) const fn screen_cell(self) -> Option<ScreenCell> {
        match self {
            Self::Unavailable => None,
            Self::Exact(cell) | Self::Deferred(cell) => Some(cell),
        }
    }

    pub(crate) const fn exact_screen_cell(self) -> Option<ScreenCell> {
        match self {
            Self::Exact(cell) => Some(cell),
            Self::Deferred(_) | Self::Unavailable => None,
        }
    }

    pub(crate) const fn requires_exact_refresh(self) -> bool {
        matches!(self, Self::Deferred(_))
    }
}

/// The observation-time cursor facts retained by reducer state.
///
/// The event layer is responsible for collapsing raw host details such as
/// conceal, `screenpos()`, and cached deltas into this display-space contract
/// before constructing [`CursorObservation`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CursorObservation {
    buffer_line: BufferLine,
    cell: ObservedCell,
}

impl CursorObservation {
    pub(crate) const fn new(buffer_line: BufferLine, cell: ObservedCell) -> Self {
        Self { buffer_line, cell }
    }

    pub(crate) const fn buffer_line(self) -> BufferLine {
        self.buffer_line
    }

    pub(crate) const fn cell(self) -> ObservedCell {
        self.cell
    }

    pub(crate) const fn screen_cell(self) -> Option<ScreenCell> {
        self.cell.screen_cell()
    }

    pub(crate) const fn exact_screen_cell(self) -> Option<ScreenCell> {
        self.cell.exact_screen_cell()
    }

    pub(crate) const fn requires_exact_refresh(self) -> bool {
        self.cell.requires_exact_refresh()
    }
}
