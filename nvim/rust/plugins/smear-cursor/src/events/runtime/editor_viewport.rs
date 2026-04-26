//! Canonical shell-side owner of editor viewport reads and command-row math.

use super::telemetry::record_command_row_read;
use super::telemetry::record_editor_bounds_read;
use crate::host::EditorViewportPort;
use crate::host::NeovimHost;
use crate::position::ViewportBounds;
use nvim_oxi::Result;

/// A live editor viewport snapshot used to derive command-row and bounds facts.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct EditorViewportSnapshot {
    lines: i64,
    cmdheight: i64,
    columns: i64,
}

impl EditorViewportSnapshot {
    fn read_live_with(
        host: &impl EditorViewportPort,
        read_kind: EditorViewportReadKind,
    ) -> Result<Self> {
        read_kind.record();
        let options = host.editor_viewport_options()?;
        Ok(Self {
            lines: options.lines(),
            cmdheight: options.cmdheight(),
            columns: options.columns(),
        })
    }

    pub(crate) fn max_row(self) -> i64 {
        self.command_row()
    }

    pub(crate) fn max_col(self) -> i64 {
        self.columns.max(1)
    }

    pub(crate) fn command_row(self) -> i64 {
        self.lines
            .saturating_sub(self.cmdheight.max(1))
            .saturating_add(1)
            .max(1)
    }

    pub(crate) fn bounds(self) -> Option<ViewportBounds> {
        ViewportBounds::new(self.max_row(), self.max_col())
    }

    #[cfg(test)]
    pub(crate) const fn from_dimensions(lines: i64, cmdheight: i64, columns: i64) -> Self {
        Self {
            lines,
            cmdheight,
            columns,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum EditorViewportReadKind {
    EditorBounds,
    CommandRow,
}

impl EditorViewportReadKind {
    fn record(self) {
        match self {
            Self::EditorBounds => record_editor_bounds_read(),
            Self::CommandRow => record_command_row_read(),
        }
    }
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub(in crate::events) struct EditorViewportCache {
    cached: Option<EditorViewportSnapshot>,
}

impl EditorViewportCache {
    pub(in crate::events) fn read_for_bounds(&mut self) -> Result<EditorViewportSnapshot> {
        self.read_for_bounds_with(&NeovimHost)
    }

    fn read_for_bounds_with(
        &mut self,
        host: &impl EditorViewportPort,
    ) -> Result<EditorViewportSnapshot> {
        self.read(host, EditorViewportReadKind::EditorBounds)
    }

    pub(in crate::events) fn read_for_command_row(&mut self) -> Result<EditorViewportSnapshot> {
        self.read_for_command_row_with(&NeovimHost)
    }

    fn read_for_command_row_with(
        &mut self,
        host: &impl EditorViewportPort,
    ) -> Result<EditorViewportSnapshot> {
        self.read(host, EditorViewportReadKind::CommandRow)
    }

    pub(in crate::events) fn refresh(&mut self) -> Result<()> {
        self.cached = Some(EditorViewportSnapshot::read_live_with(
            &NeovimHost,
            EditorViewportReadKind::EditorBounds,
        )?);
        Ok(())
    }

    pub(in crate::events) fn invalidate(&mut self) {
        self.cached = None;
    }

    fn read(
        &mut self,
        host: &impl EditorViewportPort,
        read_kind: EditorViewportReadKind,
    ) -> Result<EditorViewportSnapshot> {
        if let Some(cached) = self.cached {
            return Ok(cached);
        }

        let viewport = EditorViewportSnapshot::read_live_with(host, read_kind)?;
        self.cached = Some(viewport);
        Ok(viewport)
    }

    #[cfg(test)]
    pub(in crate::events) fn store_for_test(&mut self, viewport: EditorViewportSnapshot) {
        self.cached = Some(viewport);
    }

    #[cfg(test)]
    pub(in crate::events) fn cached_for_test(&self) -> Option<EditorViewportSnapshot> {
        self.cached
    }
}

#[cfg(test)]
mod tests {
    use super::EditorViewportCache;
    use super::EditorViewportSnapshot;
    use crate::host::EditorViewportOptions;
    use crate::host::FakeEditorViewportPort;
    use crate::position::ViewportBounds;
    use pretty_assertions::assert_eq;

    #[test]
    fn viewport_math_matches_visible_command_row_contract() {
        let viewport = EditorViewportSnapshot::from_dimensions(42, 0, 120);

        assert_eq!(viewport.command_row(), 42);
        assert_eq!(viewport.max_row(), 42);
        assert_eq!(viewport.max_col(), 120);
        assert_eq!(
            viewport.bounds(),
            Some(ViewportBounds::new(42, 120).expect("positive viewport bounds"))
        );
    }

    #[test]
    fn viewport_math_clamps_invalid_dimensions() {
        let viewport = EditorViewportSnapshot::from_dimensions(0, 3, 0);

        assert_eq!(viewport.command_row(), 1);
        assert_eq!(viewport.max_row(), 1);
        assert_eq!(viewport.max_col(), 1);
        assert_eq!(
            viewport.bounds(),
            Some(ViewportBounds::new(1, 1).expect("positive viewport bounds"))
        );
    }

    #[test]
    fn cache_invalidation_clears_only_the_cached_viewport() {
        let mut cache = EditorViewportCache::default();
        let viewport = EditorViewportSnapshot::from_dimensions(24, 1, 80);
        cache.store_for_test(viewport);

        assert_eq!(cache.cached_for_test(), Some(viewport));
        cache.invalidate();
        assert_eq!(cache.cached_for_test(), None);
    }

    #[test]
    fn cache_reads_live_viewport_through_host_port_once() {
        let host = FakeEditorViewportPort::default();
        let expected = EditorViewportSnapshot::from_dimensions(24, 1, 80);
        host.push_editor_viewport_options(EditorViewportOptions::new(
            /*lines*/ 24, /*cmdheight*/ 1, /*columns*/ 80,
        ));
        let mut cache = EditorViewportCache::default();

        let first = cache
            .read_for_bounds_with(&host)
            .expect("fake viewport options should produce a snapshot");
        let second = cache
            .read_for_command_row_with(&host)
            .expect("cached viewport should not call the fake host again");

        assert_eq!((first, second, host.calls()), (expected, expected, 1));
    }
}
