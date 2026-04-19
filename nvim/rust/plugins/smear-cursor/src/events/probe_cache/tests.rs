pub(super) use super::CachedConcealDelta;
pub(super) use super::CachedConcealRegions;
pub(super) use super::ConcealCacheLookup;
pub(super) use super::ConcealDeltaCacheKey;
pub(super) use super::ConcealDeltaCacheLookup;
pub(super) use super::ConcealRegion;
pub(super) use super::ConcealScreenCellCacheKey;
pub(super) use super::ConcealScreenCellCacheLookup;
pub(super) use super::CursorColorCacheLookup;
pub(super) use super::CursorTextContextCacheKey;
pub(super) use super::CursorTextContextCacheLookup;
pub(super) use super::ProbeCacheState;
pub(super) use crate::core::state::CursorColorSample;
pub(super) use crate::core::state::CursorTextContext;
pub(super) use crate::core::state::ObservedTextRow;
pub(super) use crate::core::types::Generation;
pub(super) use crate::test_support::conceal_key;
pub(super) use crate::test_support::conceal_region;
pub(super) use crate::test_support::conceal_window_state;
pub(super) use crate::test_support::cursor;
pub(super) use crate::test_support::cursor_color_probe_witness_with_cache_generation as witness_with_cache_generation;
pub(super) use crate::test_support::proptest::cache_key_mutation_axis;
pub(super) use crate::test_support::proptest::pure_config;
pub(super) use proptest::collection::vec;
use proptest::prelude::*;
pub(super) use std::sync::Arc;

mod boundaries;
mod conceal_deltas;
mod conceal_regions;
mod conceal_screen_cells;
mod cursor_text_context;
mod invalidation;

pub(super) const CURSOR_TEXT_CONTEXT_AXIS_COUNT: usize = 4;

pub(super) fn cursor_text_context_key(
    buffer_handle: i64,
    changedtick: u64,
    cursor_line: i64,
    tracked_line: Option<i64>,
) -> CursorTextContextCacheKey {
    CursorTextContextCacheKey::new(buffer_handle, changedtick, cursor_line, tracked_line)
}

fn observed_row(text: &str) -> ObservedTextRow {
    ObservedTextRow::new(text.to_string())
}

pub(super) fn cursor_text_context(
    buffer_handle: i64,
    changedtick: u64,
    cursor_line: i64,
    nearby_rows: Vec<ObservedTextRow>,
    tracked_nearby_rows: Option<Vec<ObservedTextRow>>,
) -> CursorTextContext {
    CursorTextContext::new(
        buffer_handle,
        changedtick,
        cursor_line,
        nearby_rows,
        tracked_nearby_rows,
    )
}

pub(super) fn concealcursor_strategy() -> BoxedStrategy<String> {
    prop_oneof![
        Just(String::new()),
        Just("n".to_string()),
        Just("i".to_string()),
        Just("v".to_string()),
        Just("nvc".to_string()),
    ]
    .boxed()
}

pub(super) fn cursor_color_sample_strategy() -> BoxedStrategy<Option<CursorColorSample>> {
    proptest::option::of(any::<u32>().prop_map(CursorColorSample::new)).boxed()
}

pub(super) fn observed_rows_strategy(max_rows: usize) -> BoxedStrategy<Vec<ObservedTextRow>> {
    vec(any::<u16>(), 0..=max_rows)
        .prop_map(|values| {
            values
                .into_iter()
                .map(|value| observed_row(&format!("row-{value}")))
                .collect()
        })
        .boxed()
}

pub(super) fn conceal_regions_strategy() -> BoxedStrategy<Arc<[ConcealRegion]>> {
    vec((1_i64..128, 0_i64..4, any::<i64>(), 0_i64..4), 0..=4)
        .prop_map(|entries| {
            entries
                .into_iter()
                .map(|(start_col1, span, match_id, replacement_width)| {
                    conceal_region(
                        start_col1,
                        start_col1.saturating_add(span),
                        match_id,
                        replacement_width,
                    )
                })
                .collect::<Vec<_>>()
                .into()
        })
        .boxed()
}

#[derive(Clone, Copy, Debug)]
pub(super) enum BoundaryEvent {
    CursorColorObservationBoundary,
    CursorColorColorschemeChange,
    ConcealReadBoundary,
    Reset,
}

pub(super) fn boundary_event_strategy() -> BoxedStrategy<BoundaryEvent> {
    prop_oneof![
        Just(BoundaryEvent::CursorColorObservationBoundary),
        Just(BoundaryEvent::CursorColorColorschemeChange),
        Just(BoundaryEvent::ConcealReadBoundary),
        Just(BoundaryEvent::Reset),
    ]
    .boxed()
}
