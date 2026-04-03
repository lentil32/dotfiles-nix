use crate::core::state::CursorColorProbeWitness;
use crate::core::types::CursorCol;
use crate::core::types::CursorPosition;
use crate::core::types::CursorRow;
use crate::core::types::Generation;
use crate::core::types::ViewportSnapshot;
use crate::events::ConcealScreenCellView;
use crate::events::probe_cache::ConcealCacheKey;
use crate::events::probe_cache::ConcealRegion;
use crate::events::probe_cache::ConcealWindowState;
use crate::types::ScreenCell;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ConcealScreenCellViewBuilder {
    window_row: i64,
    window_col: i64,
    window_width: i64,
    window_height: i64,
    topline: i64,
    leftcol: i64,
    textoff: i64,
}

impl ConcealScreenCellViewBuilder {
    pub(crate) const fn new() -> Self {
        Self {
            window_row: 0,
            window_col: 0,
            window_width: 0,
            window_height: 0,
            topline: 0,
            leftcol: 0,
            textoff: 0,
        }
    }

    pub(crate) const fn from_view(view: ConcealScreenCellView) -> Self {
        let (window_row, window_col, window_width, window_height, topline, leftcol, textoff) =
            view.fixture_parts();
        Self {
            window_row,
            window_col,
            window_width,
            window_height,
            topline,
            leftcol,
            textoff,
        }
    }

    pub(crate) const fn window_origin(mut self, window_row: i64, window_col: i64) -> Self {
        self.window_row = window_row;
        self.window_col = window_col;
        self
    }

    pub(crate) const fn window_row(mut self, window_row: i64) -> Self {
        self.window_row = window_row;
        self
    }

    pub(crate) const fn window_col(mut self, window_col: i64) -> Self {
        self.window_col = window_col;
        self
    }

    pub(crate) const fn window_size(mut self, window_width: i64, window_height: i64) -> Self {
        self.window_width = window_width;
        self.window_height = window_height;
        self
    }

    pub(crate) const fn window_width(mut self, window_width: i64) -> Self {
        self.window_width = window_width;
        self
    }

    pub(crate) const fn window_height(mut self, window_height: i64) -> Self {
        self.window_height = window_height;
        self
    }

    pub(crate) const fn viewport(mut self, topline: i64, leftcol: i64, textoff: i64) -> Self {
        self.topline = topline;
        self.leftcol = leftcol;
        self.textoff = textoff;
        self
    }

    pub(crate) const fn topline(mut self, topline: i64) -> Self {
        self.topline = topline;
        self
    }

    pub(crate) const fn leftcol(mut self, leftcol: i64) -> Self {
        self.leftcol = leftcol;
        self
    }

    pub(crate) const fn textoff(mut self, textoff: i64) -> Self {
        self.textoff = textoff;
        self
    }

    pub(crate) const fn build(self) -> ConcealScreenCellView {
        ConcealScreenCellView::new(
            self.window_row,
            self.window_col,
            self.window_width,
            self.window_height,
            self.topline,
            self.leftcol,
            self.textoff,
        )
    }
}

pub(crate) fn cursor(row: u32, col: u32) -> CursorPosition {
    CursorPosition {
        row: CursorRow(row),
        col: CursorCol(col),
    }
}

pub(crate) fn cursor_color_probe_witness(
    window_handle: i64,
    buffer_handle: i64,
    changedtick: u64,
    mode: &str,
    cursor_position: Option<CursorPosition>,
    colorscheme_generation: u64,
) -> CursorColorProbeWitness {
    cursor_color_probe_witness_with_cache_generation(
        window_handle,
        buffer_handle,
        changedtick,
        mode,
        cursor_position,
        colorscheme_generation,
        0,
    )
}

pub(crate) fn cursor_color_probe_witness_with_cache_generation(
    window_handle: i64,
    buffer_handle: i64,
    changedtick: u64,
    mode: &str,
    cursor_position: Option<CursorPosition>,
    colorscheme_generation: u64,
    cache_generation: u64,
) -> CursorColorProbeWitness {
    CursorColorProbeWitness::new(
        window_handle,
        buffer_handle,
        changedtick,
        mode.to_string(),
        cursor_position,
        Generation::new(colorscheme_generation),
        Generation::new(cache_generation),
    )
}

pub(crate) fn sparse_probe_cells(viewport: ViewportSnapshot, count: usize) -> Vec<ScreenCell> {
    let width = i64::from(viewport.max_col.value());
    (0..count)
        .map(|index| {
            let index = i64::try_from(index).expect("probe cell index");
            let row = index / width + 1;
            let col = index % width + 1;
            ScreenCell::new(row, col).expect("probe cell")
        })
        .collect()
}

pub(crate) fn conceal_region(
    start_col1: i64,
    end_col1: i64,
    match_id: i64,
    replacement_width: i64,
) -> ConcealRegion {
    ConcealRegion {
        start_col1,
        end_col1,
        match_id,
        replacement_width,
    }
}

pub(crate) fn conceal_window_state(
    conceallevel: i64,
    concealcursor: impl Into<String>,
) -> ConcealWindowState {
    ConcealWindowState::new(conceallevel, concealcursor)
}

pub(crate) fn conceal_key(
    buffer_handle: i64,
    changedtick: u64,
    line: usize,
    conceallevel: i64,
    concealcursor: impl Into<String>,
) -> ConcealCacheKey {
    ConcealCacheKey::new(
        buffer_handle,
        changedtick,
        line,
        conceal_window_state(conceallevel, concealcursor),
    )
}

pub(crate) fn options_dict<'a>(entries: impl IntoIterator<Item = (&'a str, Object)>) -> Dictionary {
    let mut opts = Dictionary::new();
    for (key, value) in entries {
        opts.insert(key, value);
    }
    opts
}
