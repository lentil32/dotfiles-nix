use super::CachedConcealDriftHint;
use super::ConcealScreenCellView;
use super::WrappedScreenCellLayout;
use super::apply_conceal_delta;
use super::cached_conceal_drift_hint_from_regions_and_delta;
use super::conceal_delta_for_regions;
use super::concealcursor_allows_mode;
use super::merge_conceal_region;
use crate::events::probe_cache::ConcealRegion;
use crate::test_support::ConcealScreenCellViewBuilder;
use crate::test_support::conceal_key;
use crate::test_support::conceal_region;
use crate::test_support::proptest::cache_key_mutation_axis;
use crate::test_support::proptest::pure_config;
use proptest::collection::vec;
use proptest::prelude::*;
use std::collections::BTreeMap;

const SCREEN_CELL_VIEW_AXIS_COUNT: usize = 8;
const DELTA_CACHE_VIEW_AXIS_COUNT: usize = 7;
const CONCEAL_KEY_AXIS_COUNT: usize = 2;

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
    raw_cell: (i64, i64),
    regions: Vec<ConcealRegion>,
    cells_by_col1: BTreeMap<i64, (i64, i64)>,
    expected_delta: i64,
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

fn different_concealcursor(concealcursor: &str) -> String {
    let without_normal: String = concealcursor.chars().filter(|&ch| ch != 'n').collect();
    if without_normal != concealcursor {
        without_normal
    } else {
        format!("{concealcursor}n")
    }
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

fn same_row_advance((row, col): (i64, i64), delta: i64) -> (i64, i64) {
    (row, col.saturating_add(delta))
}

fn wrapped_advance(
    layout: WrappedScreenCellLayout,
    mut cell: (i64, i64),
    mut delta: i64,
) -> (i64, i64) {
    while delta > 0 {
        let cells_to_row_end = layout.text_end_col().saturating_sub(cell.1);
        if delta <= cells_to_row_end {
            cell.1 = cell.1.saturating_add(delta);
            return cell;
        }

        delta = delta.saturating_sub(cells_to_row_end.saturating_add(1));
        cell.0 = cell.0.saturating_add(1);
        cell.1 = layout.text_start_col;
    }

    cell
}

fn build_conceal_delta_fixture(
    specs: &[ConcealDeltaRegionSpec],
    start_cell: (i64, i64),
    mut advance: impl FnMut((i64, i64), i64) -> (i64, i64),
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

fn mutate_view(
    view: ConcealScreenCellView,
    axis: usize,
    textoff_limit: i64,
) -> ConcealScreenCellView {
    let builder = ConcealScreenCellViewBuilder::from_view(view);
    match axis {
        0 => builder
            .window_row(view.window_row.saturating_add(1))
            .build(),
        1 => builder
            .window_col(view.window_col.saturating_add(1))
            .build(),
        2 => builder
            .window_width(view.window_width.saturating_add(1))
            .build(),
        3 => builder
            .window_height(view.window_height.saturating_add(1))
            .build(),
        4 => builder.topline(view.topline.saturating_add(1)).build(),
        5 => builder.leftcol(view.leftcol.saturating_add(1)).build(),
        6 => builder
            .textoff((view.textoff.saturating_add(1)).min(textoff_limit))
            .build(),
        _ => panic!("unexpected view axis {axis}"),
    }
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_apply_conceal_delta_without_wrapping_matches_row_preserving_fallback(
        raw_row in 1_i64..256,
        raw_col in 1_i64..256,
        conceal_delta in any::<i64>(),
    ) {
        let raw_cell = (raw_row, raw_col);

        prop_assert_eq!(
            apply_conceal_delta(raw_cell, conceal_delta, None),
            (
                raw_row as f64,
                raw_col.saturating_sub(conceal_delta).max(1) as f64,
            ),
        );
    }

    #[test]
    fn prop_apply_conceal_delta_matches_wrapped_shift_left_when_view_is_valid(
        window_row in any::<i64>(),
        window_col in 1_i64..16,
        window_height in 1_i64..64,
        topline in any::<i64>(),
        leftcol in any::<i64>(),
        textoff in 0_i64..8,
        text_width in 1_i64..12,
        raw_row in 2_i64..64,
        start_offset in 0_i64..11,
        conceal_delta in 0_i64..96,
    ) {
        prop_assume!(start_offset < text_width);

        let view = ConcealScreenCellViewBuilder::new()
            .window_origin(window_row, window_col)
            .window_size(textoff.saturating_add(text_width), window_height)
            .viewport(topline, leftcol, textoff)
            .build();
        let layout = WrappedScreenCellLayout::from_view(view)
            .expect("positive window_col and text_width should produce a wrapped layout");
        let raw_cell = (
            raw_row,
            layout.text_start_col.saturating_add(start_offset),
        );
        let expected_cell = layout.shift_left(raw_cell, conceal_delta).unwrap_or((
            raw_cell.0,
            raw_cell.1.saturating_sub(conceal_delta).max(1),
        ));

        prop_assert_eq!(
            apply_conceal_delta(raw_cell, conceal_delta, Some(view)),
            (expected_cell.0 as f64, expected_cell.1 as f64),
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
    fn prop_screen_cell_cache_key_changes_for_each_effective_view_axis(
        window_handle in any::<i64>(),
        buffer_handle in any::<i64>(),
        changedtick in any::<u64>(),
        line in 0_usize..256,
        col1 in 1_i64..256,
        conceallevel in any::<i64>(),
        concealcursor in concealcursor_strategy(),
        window_row in any::<i64>(),
        window_col in any::<i64>(),
        window_width in 4_i64..128,
        window_height in 1_i64..64,
        topline in any::<i64>(),
        leftcol in any::<i64>(),
        textoff in 0_i64..3,
        axis in cache_key_mutation_axis(SCREEN_CELL_VIEW_AXIS_COUNT),
    ) {
        let conceal_key =
            conceal_key(buffer_handle, changedtick, line, conceallevel, concealcursor);
        let base_view = ConcealScreenCellViewBuilder::new()
            .window_origin(window_row, window_col)
            .window_size(window_width, window_height)
            .viewport(topline, leftcol, textoff)
            .build();
        let base_key = base_view.cache_key(window_handle, &conceal_key, col1);
        let mutated_key = match axis.index() {
            0 => base_view.cache_key(window_handle, &conceal_key, col1.saturating_add(1)),
            axis_index => mutate_view(base_view, axis_index.saturating_sub(1), window_width)
                .cache_key(window_handle, &conceal_key, col1),
        };

        prop_assert_eq!(
            &base_key,
            &base_view.cache_key(window_handle, &conceal_key, col1),
        );
        prop_assert_ne!(&base_key, &mutated_key);
    }

    #[test]
    fn prop_delta_cache_key_changes_for_each_effective_view_axis(
        window_handle in any::<i64>(),
        buffer_handle in any::<i64>(),
        changedtick in any::<u64>(),
        line in 0_usize..256,
        conceallevel in any::<i64>(),
        concealcursor in concealcursor_strategy(),
        window_row in any::<i64>(),
        window_col in any::<i64>(),
        window_width in 4_i64..128,
        window_height in 1_i64..64,
        topline in any::<i64>(),
        leftcol in any::<i64>(),
        textoff in 0_i64..3,
        axis in cache_key_mutation_axis(DELTA_CACHE_VIEW_AXIS_COUNT),
    ) {
        let conceal_key =
            conceal_key(buffer_handle, changedtick, line, conceallevel, concealcursor);
        let base_view = ConcealScreenCellViewBuilder::new()
            .window_origin(window_row, window_col)
            .window_size(window_width, window_height)
            .viewport(topline, leftcol, textoff)
            .build();
        let base_key = base_view.delta_cache_key(window_handle, &conceal_key);
        let mutated_key = mutate_view(base_view, axis.index(), window_width)
            .delta_cache_key(window_handle, &conceal_key);

        prop_assert_eq!(
            &base_key,
            &base_view.delta_cache_key(window_handle, &conceal_key),
        );
        prop_assert_ne!(&base_key, &mutated_key);
    }

    #[test]
    fn prop_conceal_cache_key_changes_for_each_window_local_conceal_axis(
        buffer_handle in any::<i64>(),
        changedtick in any::<u64>(),
        line in 0_usize..256,
        conceallevel in any::<i64>(),
        concealcursor in concealcursor_strategy(),
        axis in cache_key_mutation_axis(CONCEAL_KEY_AXIS_COUNT),
    ) {
        let base = conceal_key(
            buffer_handle,
            changedtick,
            line,
            conceallevel,
            concealcursor.clone(),
        );
        let mutated = match axis.index() {
            0 => conceal_key(
                buffer_handle,
                changedtick,
                line,
                conceallevel.saturating_add(1),
                concealcursor,
            ),
            1 => conceal_key(
                buffer_handle,
                changedtick,
                line,
                conceallevel,
                different_concealcursor(&concealcursor),
            ),
            _ => panic!("unexpected conceal key axis {}", axis.index()),
        };

        prop_assert_ne!(base, mutated);
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
        let fixture = build_conceal_delta_fixture(&specs, (row, start_col), same_row_advance);
        let delta = conceal_delta_for_regions(
            fixture.current_col1,
            fixture.raw_cell,
            &fixture.regions,
            None,
            |col1| Ok(fixture.cells_by_col1.get(&col1).copied()),
        )?;

        prop_assert_eq!(delta, Some(fixture.expected_delta));
        prop_assert_eq!(
            cached_conceal_drift_hint_from_regions_and_delta(
                fixture.current_col1,
                &fixture.regions,
                delta,
            ),
            expected_drift_hint(!fixture.regions.is_empty(), delta),
        );
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

        let view = ConcealScreenCellViewBuilder::new()
            .window_origin(1, window_col)
            .window_size(textoff.saturating_add(text_width), 32)
            .viewport(1, 0, textoff)
            .build();
        let layout = WrappedScreenCellLayout::from_view(view)
            .expect("positive window_col and text_width should produce a wrapped layout");
        let start_cell = (
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
        prop_assert_eq!(
            cached_conceal_drift_hint_from_regions_and_delta(
                fixture.current_col1,
                &fixture.regions,
                delta,
            ),
            expected_drift_hint(!fixture.regions.is_empty(), delta),
        );
    }
}
