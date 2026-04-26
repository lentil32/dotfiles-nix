use super::CachedConcealDriftHint;
use super::RawScreenposProjection;
use super::WrappedScreenCellLayout;
use super::apply_conceal_delta;
use super::cached_conceal_drift_hint_from_regions_and_delta;
use super::conceal_delta_for_regions;
use super::concealcursor_allows_mode;
use super::exact_observed_cell_from_conceal_delta;
use super::extend_concealed_regions;
use super::merge_conceal_region;
use super::projected_observed_cell_from_cached_conceal;
use crate::events::probe_cache::ConcealRegion;
use crate::host::CursorReadCall;
use crate::host::FakeCursorReadPort;
use crate::position::BufferLine;
use crate::position::ObservedCell;
use crate::position::ScreenCell;
use crate::position::SurfaceId;
use crate::position::ViewportBounds;
use crate::position::WindowSurfaceSnapshot;
use crate::test_support::conceal_region;
use crate::test_support::proptest::pure_config;
use nvim_oxi::Array;
use nvim_oxi::Object;
use pretty_assertions::assert_eq;
use proptest::collection::vec;
use proptest::prelude::*;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
enum ConcealModeFamily {
    Normal,
    Insert,
    Replace,
    Visual,
    Cmdline,
    Terminal,
}

#[derive(Clone, Debug)]
struct ModeCase {
    family: ConcealModeFamily,
    mode: &'static str,
}

#[derive(Clone, Debug)]
struct MergeCellSpec {
    col1: i64,
    match_id: i64,
    replacement_width: i64,
}

#[derive(Clone, Debug)]
struct ConcealDeltaRegionSpec {
    logical_width: i64,
    replacement_width: i64,
    raw_width: i64,
    gap_width: i64,
    match_id: i64,
}

#[derive(Clone, Debug)]
struct ConcealDeltaFixture {
    current_col1: i64,
    raw_cell: ScreenCell,
    regions: Vec<ConcealRegion>,
    cells_by_col1: BTreeMap<i64, ScreenCell>,
    expected_delta: i64,
}

fn screen_cell(row: i64, col: i64) -> ScreenCell {
    ScreenCell::new(row, col).expect("test screen cells should stay one-based")
}

fn synconcealed_object(concealed: i64, replacement: &str, match_id: i64) -> Object {
    Object::from(Array::from_iter([
        Object::from(concealed),
        Object::from(replacement),
        Object::from(match_id),
    ]))
}

fn surface_snapshot(
    window_row: i64,
    window_col: i64,
    window_width: i64,
    window_height: i64,
    topline: i64,
    leftcol: i64,
    textoff: i64,
) -> WindowSurfaceSnapshot {
    surface_snapshot_with_handles(
        (11, 17),
        (window_row, window_col),
        (window_width, window_height),
        topline,
        leftcol,
        textoff,
    )
}

fn surface_snapshot_with_handles(
    handles: (i64, i64),
    window_origin: (i64, i64),
    window_size: (i64, i64),
    topline: i64,
    leftcol: i64,
    textoff: i64,
) -> WindowSurfaceSnapshot {
    WindowSurfaceSnapshot::new(
        SurfaceId::new(handles.0, handles.1).expect("positive handles"),
        BufferLine::new(topline).expect("positive top buffer line"),
        u32::try_from(leftcol).expect("non-negative left column"),
        u32::try_from(textoff).expect("non-negative text offset"),
        screen_cell(window_origin.0, window_origin.1),
        ViewportBounds::new(window_size.1, window_size.0).expect("positive viewport bounds"),
    )
}

fn concealcursor_strategy() -> BoxedStrategy<String> {
    (
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        vec(prop_oneof![Just('x'), Just('y'), Just('z')], 0..=3),
    )
        .prop_map(
            |(allow_normal, allow_insert, allow_visual, allow_cmdline, noise)| {
                let mut concealcursor = String::new();
                if allow_normal {
                    concealcursor.push('n');
                }
                if allow_insert {
                    concealcursor.push('i');
                }
                if allow_visual {
                    concealcursor.push('v');
                }
                if allow_cmdline {
                    concealcursor.push('c');
                }
                concealcursor.extend(noise);
                concealcursor
            },
        )
        .boxed()
}

fn mode_case_strategy() -> BoxedStrategy<ModeCase> {
    prop_oneof![
        Just(ModeCase {
            family: ConcealModeFamily::Normal,
            mode: "n",
        }),
        Just(ModeCase {
            family: ConcealModeFamily::Normal,
            mode: "no",
        }),
        Just(ModeCase {
            family: ConcealModeFamily::Insert,
            mode: "i",
        }),
        Just(ModeCase {
            family: ConcealModeFamily::Insert,
            mode: "ic",
        }),
        Just(ModeCase {
            family: ConcealModeFamily::Replace,
            mode: "R",
        }),
        Just(ModeCase {
            family: ConcealModeFamily::Replace,
            mode: "Rc",
        }),
        Just(ModeCase {
            family: ConcealModeFamily::Visual,
            mode: "v",
        }),
        Just(ModeCase {
            family: ConcealModeFamily::Visual,
            mode: "V",
        }),
        Just(ModeCase {
            family: ConcealModeFamily::Cmdline,
            mode: "c",
        }),
        Just(ModeCase {
            family: ConcealModeFamily::Cmdline,
            mode: "cv",
        }),
        Just(ModeCase {
            family: ConcealModeFamily::Terminal,
            mode: "t",
        }),
    ]
    .boxed()
}

fn merge_cell_specs_strategy() -> BoxedStrategy<Vec<MergeCellSpec>> {
    vec((1_i64..=3, any::<i64>(), 0_i64..=4), 0..=12)
        .prop_map(|steps| {
            let mut col1 = 0_i64;
            steps
                .into_iter()
                .map(|(gap, match_id, replacement_width)| {
                    col1 = col1.saturating_add(gap);
                    MergeCellSpec {
                        col1,
                        match_id,
                        replacement_width,
                    }
                })
                .collect()
        })
        .boxed()
}

fn conceal_delta_region_specs_strategy() -> BoxedStrategy<Vec<ConcealDeltaRegionSpec>> {
    vec(
        (1_i64..=3, 0_i64..=4, 0_i64..=8, 1_i64..=4, any::<i64>()),
        0..=8,
    )
    .prop_map(|entries| {
        entries
            .into_iter()
            .map(
                |(logical_width, replacement_width, raw_width, gap_width, match_id)| {
                    ConcealDeltaRegionSpec {
                        logical_width,
                        replacement_width,
                        raw_width,
                        gap_width,
                        match_id,
                    }
                },
            )
            .collect()
    })
    .boxed()
}

fn build_merge_regions_reference(specs: &[MergeCellSpec]) -> Vec<ConcealRegion> {
    let mut regions: Vec<ConcealRegion> = Vec::new();
    for spec in specs {
        if let Some(last) = regions.last_mut()
            && last.match_id == spec.match_id
            && last.replacement_width == spec.replacement_width
            && last.end_col1.saturating_add(1) == spec.col1
        {
            last.end_col1 = spec.col1;
            continue;
        }

        regions.push(conceal_region(
            spec.col1,
            spec.col1,
            spec.match_id,
            spec.replacement_width,
        ));
    }
    regions
}

fn merge_regions(specs: &[MergeCellSpec]) -> Vec<ConcealRegion> {
    let mut regions = Vec::new();
    for spec in specs {
        merge_conceal_region(
            &mut regions,
            spec.col1,
            spec.match_id,
            spec.replacement_width,
        );
    }
    regions
}

fn same_row_advance(cell: ScreenCell, delta: i64) -> ScreenCell {
    screen_cell(cell.row(), cell.col().saturating_add(delta))
}

fn wrapped_advance(
    layout: WrappedScreenCellLayout,
    cell: ScreenCell,
    mut delta: i64,
) -> ScreenCell {
    let mut row = cell.row();
    let mut col = cell.col();
    while delta > 0 {
        let cells_to_row_end = layout.text_end_col().saturating_sub(col);
        if delta <= cells_to_row_end {
            return screen_cell(row, col.saturating_add(delta));
        }

        delta = delta.saturating_sub(cells_to_row_end.saturating_add(1));
        row = row.saturating_add(1);
        col = layout.text_start_col;
    }

    screen_cell(row, col)
}

fn build_conceal_delta_fixture(
    specs: &[ConcealDeltaRegionSpec],
    start_cell: ScreenCell,
    mut advance: impl FnMut(ScreenCell, i64) -> ScreenCell,
) -> ConcealDeltaFixture {
    let mut current_col1 = 1_i64;
    let mut cursor = start_cell;
    let mut expected_delta = 0_i64;
    let mut regions = Vec::new();
    let mut cells_by_col1 = BTreeMap::new();

    for spec in specs {
        let region_start = current_col1;
        let region_end = region_start
            .saturating_add(spec.logical_width)
            .saturating_sub(1);
        regions.push(conceal_region(
            region_start,
            region_end,
            spec.match_id,
            spec.replacement_width,
        ));
        cells_by_col1.insert(region_start, cursor);
        cursor = advance(cursor, spec.raw_width);
        cells_by_col1.insert(region_end.saturating_add(1), cursor);
        cursor = advance(cursor, spec.gap_width);
        current_col1 = region_end.saturating_add(2);
        expected_delta = expected_delta
            .saturating_add(spec.raw_width.saturating_sub(spec.replacement_width).max(0));
    }

    ConcealDeltaFixture {
        current_col1,
        raw_cell: cursor,
        regions,
        cells_by_col1,
        expected_delta,
    }
}

fn expected_concealcursor_allows_mode(concealcursor: &str, case: &ModeCase) -> bool {
    match case.family {
        ConcealModeFamily::Normal => concealcursor.contains('n'),
        ConcealModeFamily::Insert | ConcealModeFamily::Replace => concealcursor.contains('i'),
        ConcealModeFamily::Visual => concealcursor.contains('v'),
        ConcealModeFamily::Cmdline => concealcursor.contains('c'),
        ConcealModeFamily::Terminal => false,
    }
}

fn expected_drift_hint(
    has_prior_region: bool,
    cached_delta: Option<i64>,
) -> CachedConcealDriftHint {
    if !has_prior_region {
        return CachedConcealDriftHint::NoDrift;
    }

    match cached_delta {
        Some(delta) if delta > 0 => CachedConcealDriftHint::Drifted,
        Some(_) => CachedConcealDriftHint::NoDrift,
        None => CachedConcealDriftHint::Unknown,
    }
}

#[test]
fn conceal_region_scan_reads_synconcealed_and_display_width_through_cursor_read_port() {
    let host = FakeCursorReadPort::default();
    host.push_synconcealed(synconcealed_object(1, "xx", 91));
    host.push_string_display_width(Object::from(2_i64));
    let mut regions = Vec::new();

    extend_concealed_regions(&host, 4, 3, 3, &mut regions)
        .expect("conceal region scan should use fake host reads");

    assert_eq!(regions, vec![conceal_region(3, 3, 91, 2)]);
    assert_eq!(
        host.calls(),
        vec![
            CursorReadCall::Synconcealed { line: 4, col1: 3 },
            CursorReadCall::StringDisplayWidth {
                text: "xx".to_string(),
            },
        ],
    );
}

#[test]
fn oil_style_concealed_prefix_projects_fast_path_into_display_space() {
    let surface = surface_snapshot(7, 11, 80, 24, 23, 0, 0);
    let raw_cell = screen_cell(7, 18);
    let regions = vec![conceal_region(1, 5, 91, 0)];

    assert_eq!(
        projected_observed_cell_from_cached_conceal(10, raw_cell, &regions, Some(5), Some(surface)),
        RawScreenposProjection::Projected {
            observed_cell: ObservedCell::Deferred(screen_cell(7, 13)),
            used_cached_conceal: true,
        },
    );
}

#[test]
fn cached_conceal_fast_path_marks_zero_drift_reuse_as_deferred() {
    let surface = surface_snapshot(7, 11, 80, 24, 23, 0, 0);
    let raw_cell = screen_cell(7, 18);
    let regions = vec![conceal_region(1, 5, 91, 5)];

    assert_eq!(
        projected_observed_cell_from_cached_conceal(10, raw_cell, &regions, Some(0), Some(surface),),
        RawScreenposProjection::Projected {
            observed_cell: ObservedCell::Deferred(raw_cell),
            used_cached_conceal: true,
        },
    );
}

#[test]
fn exact_conceal_projection_returns_unavailable_when_no_projected_cell_can_be_computed() {
    let raw_cell = screen_cell(7, 18);
    let surface = surface_snapshot(7, 11, 5, 24, 23, 0, 5);

    assert_eq!(
        exact_observed_cell_from_conceal_delta(raw_cell, None, Some(surface)),
        ObservedCell::Unavailable,
    );
}

#[test]
fn exact_conceal_projection_returns_unavailable_when_wrapped_shift_cannot_apply_known_delta() {
    let raw_cell = screen_cell(7, 18);
    let surface = surface_snapshot(7, 11, 5, 24, 23, 0, 5);

    assert_eq!(
        exact_observed_cell_from_conceal_delta(raw_cell, Some(5), Some(surface)),
        ObservedCell::Unavailable,
    );
}

#[test]
fn exact_conceal_projection_stays_in_display_space_when_delta_is_known() {
    let raw_cell = screen_cell(7, 18);
    let surface = surface_snapshot(7, 11, 80, 24, 23, 0, 0);

    assert_eq!(
        exact_observed_cell_from_conceal_delta(raw_cell, Some(5), Some(surface)),
        ObservedCell::Exact(screen_cell(7, 13)),
    );
}

#[test]
fn cached_conceal_fast_path_requests_exact_projection_when_wrapped_shift_cannot_apply_delta() {
    let surface = surface_snapshot(7, 11, 5, 24, 23, 0, 5);
    let raw_cell = screen_cell(7, 18);
    let regions = vec![conceal_region(1, 5, 91, 0)];

    assert_eq!(
        projected_observed_cell_from_cached_conceal(10, raw_cell, &regions, Some(5), Some(surface)),
        RawScreenposProjection::NeedsExactProjection,
    );
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_apply_conceal_delta_without_wrapping_matches_row_preserving_fallback(
        raw_row in 1_i64..256,
        raw_col in 1_i64..256,
        conceal_delta in any::<i64>(),
    ) {
        let raw_cell = screen_cell(raw_row, raw_col);

        prop_assert_eq!(
            apply_conceal_delta(raw_cell, conceal_delta, None),
            Some(screen_cell(
                raw_cell.row(),
                raw_cell.col().saturating_sub(conceal_delta).max(1),
            )),
        );
    }

    #[test]
    fn prop_apply_conceal_delta_matches_wrapped_shift_left_when_surface_is_valid(
        window_row in 1_i64..64,
        window_col in 1_i64..16,
        window_height in 1_i64..64,
        topline in 1_i64..512,
        leftcol in 0_i64..128,
        textoff in 0_i64..8,
        text_width in 1_i64..12,
        raw_row in 2_i64..64,
        start_offset in 0_i64..11,
        conceal_delta in 0_i64..96,
    ) {
        prop_assume!(start_offset < text_width);

        let surface_snapshot = surface_snapshot(
            window_row,
            window_col,
            textoff.saturating_add(text_width),
            window_height,
            topline,
            leftcol,
            textoff,
        );
        let layout = WrappedScreenCellLayout::from_surface(surface_snapshot)
            .expect("positive window_col and text_width should produce a wrapped layout");
        let raw_cell = screen_cell(
            raw_row,
            layout.text_start_col.saturating_add(start_offset),
        );
        let expected_cell = layout.shift_left(raw_cell, conceal_delta);

        prop_assert_eq!(
            apply_conceal_delta(raw_cell, conceal_delta, Some(surface_snapshot)),
            expected_cell,
        );
    }

    #[test]
    fn prop_cached_conceal_drift_hint_depends_only_on_prior_regions_and_cached_delta(
        current_col1 in 2_i64..64,
        has_prior_region in any::<bool>(),
        has_non_prior_region in any::<bool>(),
        cached_delta in proptest::option::of(any::<i64>()),
    ) {
        let mut regions = Vec::new();
        if has_prior_region {
            regions.push(conceal_region(
                current_col1.saturating_sub(1),
                current_col1.saturating_sub(1),
                11,
                0,
            ));
        }
        if has_non_prior_region {
            regions.push(conceal_region(current_col1, current_col1, 12, 1));
        }

        prop_assert_eq!(
            cached_conceal_drift_hint_from_regions_and_delta(current_col1, &regions, cached_delta),
            expected_drift_hint(has_prior_region, cached_delta),
        );
    }

    #[test]
    fn prop_merge_conceal_region_matches_run_reference_and_partitioned_merges(
        specs in merge_cell_specs_strategy(),
        split_index in 0_usize..=12,
    ) {
        prop_assume!(split_index <= specs.len());

        let merged = merge_regions(&specs);
        let expected = build_merge_regions_reference(&specs);
        let mut partitioned = merge_regions(&specs[..split_index]);
        for spec in &specs[split_index..] {
            merge_conceal_region(
                &mut partitioned,
                spec.col1,
                spec.match_id,
                spec.replacement_width,
            );
        }

        prop_assert_eq!(&merged, &expected);
        prop_assert_eq!(&partitioned, &expected);
    }

    #[test]
    fn prop_concealcursor_allows_only_its_expected_mode_families(
        concealcursor in concealcursor_strategy(),
        case in mode_case_strategy(),
    ) {
        prop_assert_eq!(
            concealcursor_allows_mode(&concealcursor, case.mode),
            expected_concealcursor_allows_mode(&concealcursor, &case),
        );
    }

    #[test]
    fn prop_conceal_delta_for_regions_accumulates_same_row_drift_exactly(
        specs in conceal_delta_region_specs_strategy(),
        row in 1_i64..64,
        start_col in 1_i64..48,
    ) {
        let fixture = build_conceal_delta_fixture(&specs, screen_cell(row, start_col), same_row_advance);
        let delta = conceal_delta_for_regions(
            fixture.current_col1,
            fixture.raw_cell,
            &fixture.regions,
            None,
            |col1| Ok(fixture.cells_by_col1.get(&col1).copied()),
        )?;

        prop_assert_eq!(delta, Some(fixture.expected_delta));
    }

    #[test]
    fn prop_conceal_delta_for_regions_accumulates_wrapped_drift_exactly(
        window_col in 1_i64..16,
        textoff in 0_i64..6,
        text_width in 1_i64..8,
        start_row in 2_i64..16,
        start_offset in 0_i64..7,
        specs in conceal_delta_region_specs_strategy(),
    ) {
        prop_assume!(start_offset < text_width);

        let surface_snapshot = surface_snapshot(1, window_col, textoff.saturating_add(text_width), 32, 1, 0, textoff);
        let layout = WrappedScreenCellLayout::from_surface(surface_snapshot)
            .expect("positive window_col and text_width should produce a wrapped layout");
        let start_cell = screen_cell(
            start_row,
            layout.text_start_col.saturating_add(start_offset),
        );
        let fixture = build_conceal_delta_fixture(&specs, start_cell, |cell, delta| {
            wrapped_advance(layout, cell, delta)
        });
        let delta = conceal_delta_for_regions(
            fixture.current_col1,
            fixture.raw_cell,
            &fixture.regions,
            Some(layout),
            |col1| Ok(fixture.cells_by_col1.get(&col1).copied()),
        )?;

        prop_assert_eq!(delta, Some(fixture.expected_delta));
    }
}
