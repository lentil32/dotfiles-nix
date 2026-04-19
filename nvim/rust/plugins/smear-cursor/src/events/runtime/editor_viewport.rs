use super::telemetry::record_command_row_read;
use super::telemetry::record_editor_bounds_read;
use nvim_oxi::Result;
use nvim_oxi::api;
use nvim_oxi::api::opts::OptionOpts;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct EditorViewport {
    lines: i64,
    cmdheight: i64,
    columns: i64,
}

impl EditorViewport {
    fn read_live(read_kind: EditorViewportReadKind) -> Result<Self> {
        read_kind.record();
        let opts = OptionOpts::builder().build();
        let lines: i64 = api::get_option_value("lines", &opts)?;
        let cmdheight: i64 = api::get_option_value("cmdheight", &opts)?;
        let columns: i64 = api::get_option_value("columns", &opts)?;

        Ok(Self {
            lines,
            cmdheight,
            columns,
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
    cached: Option<EditorViewport>,
}

impl EditorViewportCache {
    pub(in crate::events) fn read_for_bounds(&mut self) -> Result<EditorViewport> {
        self.read(EditorViewportReadKind::EditorBounds)
    }

    pub(in crate::events) fn read_for_command_row(&mut self) -> Result<EditorViewport> {
        self.read(EditorViewportReadKind::CommandRow)
    }

    pub(in crate::events) fn refresh(&mut self) -> Result<()> {
        self.cached = Some(EditorViewport::read_live(
            EditorViewportReadKind::EditorBounds,
        )?);
        Ok(())
    }

    pub(in crate::events) fn invalidate(&mut self) {
        self.cached = None;
    }

    fn read(&mut self, read_kind: EditorViewportReadKind) -> Result<EditorViewport> {
        if let Some(cached) = self.cached {
            return Ok(cached);
        }

        let viewport = EditorViewport::read_live(read_kind)?;
        self.cached = Some(viewport);
        Ok(viewport)
    }

    #[cfg(test)]
    pub(in crate::events) fn store_for_test(&mut self, viewport: EditorViewport) {
        self.cached = Some(viewport);
    }

    #[cfg(test)]
    pub(in crate::events) fn cached_for_test(&self) -> Option<EditorViewport> {
        self.cached
    }
}

#[cfg(test)]
mod tests {
    use super::EditorViewport;
    use super::EditorViewportCache;
    use pretty_assertions::assert_eq;

    #[test]
    fn viewport_math_matches_visible_command_row_contract() {
        let viewport = EditorViewport::from_dimensions(42, 0, 120);

        assert_eq!(viewport.command_row(), 42);
        assert_eq!(viewport.max_row(), 42);
        assert_eq!(viewport.max_col(), 120);
    }

    #[test]
    fn viewport_math_clamps_invalid_dimensions() {
        let viewport = EditorViewport::from_dimensions(0, 3, 0);

        assert_eq!(viewport.command_row(), 1);
        assert_eq!(viewport.max_row(), 1);
        assert_eq!(viewport.max_col(), 1);
    }

    #[test]
    fn cache_invalidation_clears_only_the_cached_viewport() {
        let mut cache = EditorViewportCache::default();
        let viewport = EditorViewport::from_dimensions(24, 1, 80);
        cache.store_for_test(viewport);

        assert_eq!(cache.cached_for_test(), Some(viewport));
        cache.invalidate();
        assert_eq!(cache.cached_for_test(), None);
    }
}
