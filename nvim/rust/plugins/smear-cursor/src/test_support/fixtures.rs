use crate::core::state::CursorColorProbeWitness;
use crate::core::types::Generation;
use crate::events::probe_cache::ConcealCacheKey;
use crate::events::probe_cache::ConcealRegion;
use crate::events::probe_cache::ConcealWindowState;
use crate::position::ScreenCell;
use crate::position::ViewportBounds;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;

fn fixture_screen_cell(row: u32, col: u32) -> ScreenCell {
    ScreenCell::new(i64::from(row), i64::from(col)).expect("fixture screen cell")
}

pub(crate) fn cursor(row: u32, col: u32) -> ScreenCell {
    fixture_screen_cell(row, col)
}

pub(crate) fn cursor_color_probe_witness_with_cache_generation(
    window_handle: i64,
    buffer_handle: i64,
    changedtick: u64,
    mode: &str,
    cursor_position: Option<ScreenCell>,
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

pub(crate) fn sparse_probe_cells(viewport: ViewportBounds, count: usize) -> Vec<ScreenCell> {
    let width = viewport.max_col();
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
