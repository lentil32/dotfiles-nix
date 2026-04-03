#![allow(dead_code)]

use crate::animation::corners_for_cursor;
use crate::core::types::CursorCol;
use crate::core::types::CursorRow;
use crate::core::types::Generation;
use crate::core::types::TimerGeneration;
use crate::core::types::TimerId;
use crate::core::types::TimerToken;
use crate::core::types::ViewportSnapshot;
use crate::draw::WindowPlacement;
use crate::events::probe_cache::ConcealWindowState;
use crate::state::CursorLocation;
use crate::test_support::models::WindowPoolFixture;
use crate::test_support::models::WindowPoolPlacementSpec;
use crate::test_support::models::WindowPoolWindowSpec;
use crate::types::Point;
use crate::types::RenderStepSample;
use crate::types::ScreenCell;
use proptest::collection::vec;
use proptest::prelude::*;

pub(crate) const PURE_PROPTEST_CASES: u32 = 96;
pub(crate) const STATEFUL_PROPTEST_CASES: u32 = 48;
pub(crate) const DEFAULT_FLOAT_EPSILON: f64 = 1.0e-6;

const FINITE_POINT_LIMIT: f64 = 4096.0;
const SCREEN_CELL_LIMIT: i64 = 512;
const VIEWPORT_LIMIT: u32 = 512;
const WINDOW_DIMENSION_LIMIT: i64 = 256;
const WINDOW_PLACEMENT_WIDTH_LIMIT: u32 = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ModeFamily {
    Normal,
    Insert,
    Replace,
    Terminal,
    Cmdline,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ModeCase {
    family: ModeFamily,
    mode: String,
}

impl ModeCase {
    pub(crate) fn new(family: ModeFamily, mode: impl Into<String>) -> Self {
        Self {
            family,
            mode: mode.into(),
        }
    }

    pub(crate) const fn family(&self) -> ModeFamily {
        self.family
    }

    pub(crate) fn mode(&self) -> &str {
        &self.mode
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CursorShapeCase {
    Block,
    VerticalBar,
    HorizontalBar,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CursorRectFixture {
    pub(crate) position: Point,
    pub(crate) shape: CursorShapeCase,
    pub(crate) corners: [Point; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CacheKeyMutationAxis(usize);

impl CacheKeyMutationAxis {
    pub(crate) const fn index(self) -> usize {
        self.0
    }
}

pub(crate) fn pure_config() -> ProptestConfig {
    ProptestConfig::with_cases(PURE_PROPTEST_CASES)
}

pub(crate) fn stateful_config() -> ProptestConfig {
    ProptestConfig::with_cases(STATEFUL_PROPTEST_CASES)
}

pub(crate) fn approx_eq_f64(left: f64, right: f64, epsilon: f64) -> bool {
    if left == right {
        return true;
    }

    (left - right).abs() <= epsilon.max(DEFAULT_FLOAT_EPSILON)
}

pub(crate) fn approx_eq_point(left: Point, right: Point, epsilon: f64) -> bool {
    approx_eq_f64(left.row, right.row, epsilon) && approx_eq_f64(left.col, right.col, epsilon)
}

pub(crate) fn mode_family() -> BoxedStrategy<ModeFamily> {
    prop_oneof![
        Just(ModeFamily::Normal),
        Just(ModeFamily::Insert),
        Just(ModeFamily::Replace),
        Just(ModeFamily::Terminal),
        Just(ModeFamily::Cmdline),
        Just(ModeFamily::Other),
    ]
    .boxed()
}

pub(crate) const fn representative_mode(family: ModeFamily) -> &'static str {
    match family {
        ModeFamily::Normal => "n",
        ModeFamily::Insert => "i",
        ModeFamily::Replace => "R",
        ModeFamily::Terminal => "t",
        ModeFamily::Cmdline => "cv",
        ModeFamily::Other => "v",
    }
}

pub(crate) fn mode_case() -> BoxedStrategy<ModeCase> {
    prop_oneof![
        Just(ModeCase::new(ModeFamily::Normal, "n")),
        Just(ModeCase::new(ModeFamily::Normal, "no")),
        Just(ModeCase::new(ModeFamily::Insert, "i")),
        Just(ModeCase::new(ModeFamily::Insert, "ic")),
        Just(ModeCase::new(ModeFamily::Replace, "R")),
        Just(ModeCase::new(ModeFamily::Replace, "Rc")),
        Just(ModeCase::new(ModeFamily::Terminal, "t")),
        Just(ModeCase::new(ModeFamily::Cmdline, "cv")),
        Just(ModeCase::new(ModeFamily::Other, "v")),
    ]
    .boxed()
}

pub(crate) fn finite_point() -> BoxedStrategy<Point> {
    (
        -FINITE_POINT_LIMIT..FINITE_POINT_LIMIT,
        -FINITE_POINT_LIMIT..FINITE_POINT_LIMIT,
    )
        .prop_map(|(row, col)| Point { row, col })
        .boxed()
}

pub(crate) fn screen_cell() -> BoxedStrategy<ScreenCell> {
    (1_i64..=SCREEN_CELL_LIMIT, 1_i64..=SCREEN_CELL_LIMIT)
        .prop_map(|(row, col)| ScreenCell::new(row, col).expect("one-based cell strategy"))
        .boxed()
}

pub(crate) fn cursor_location() -> BoxedStrategy<CursorLocation> {
    (
        any::<i16>(),
        any::<i16>(),
        1_i64..=SCREEN_CELL_LIMIT,
        1_i64..=SCREEN_CELL_LIMIT,
        0_i64..=WINDOW_DIMENSION_LIMIT,
        0_i64..=16_i64,
        any::<i16>(),
        any::<i16>(),
        0_i64..=WINDOW_DIMENSION_LIMIT,
        0_i64..=WINDOW_DIMENSION_LIMIT,
    )
        .prop_map(
            |(
                window_handle,
                buffer_handle,
                top_row,
                line,
                left_col,
                text_offset,
                window_row,
                window_col,
                window_width,
                window_height,
            )| {
                CursorLocation::new(
                    i64::from(window_handle),
                    i64::from(buffer_handle),
                    top_row,
                    line,
                )
                .with_viewport_columns(left_col, text_offset)
                .with_window_origin(i64::from(window_row), i64::from(window_col))
                .with_window_dimensions(window_width, window_height)
            },
        )
        .boxed()
}

pub(crate) fn viewport_snapshot() -> BoxedStrategy<ViewportSnapshot> {
    (1_u32..=VIEWPORT_LIMIT, 1_u32..=VIEWPORT_LIMIT)
        .prop_map(|(max_row, max_col)| {
            ViewportSnapshot::new(CursorRow(max_row), CursorCol(max_col))
        })
        .boxed()
}

pub(crate) fn generation() -> BoxedStrategy<Generation> {
    any::<u64>().prop_map(Generation::new).boxed()
}

pub(crate) fn positive_aspect_ratio() -> BoxedStrategy<f64> {
    (0.125_f64..8.0_f64).boxed()
}

pub(crate) fn positive_scale() -> BoxedStrategy<f64> {
    (0.125_f64..4.0_f64).boxed()
}

pub(crate) fn cursor_rectangle() -> BoxedStrategy<CursorRectFixture> {
    (
        1_i64..256_i64,
        1_i64..256_i64,
        prop_oneof![
            Just(CursorShapeCase::Block),
            Just(CursorShapeCase::VerticalBar),
            Just(CursorShapeCase::HorizontalBar),
        ],
    )
        .prop_map(|(row, col, shape)| {
            let position = Point {
                row: row as f64,
                col: col as f64,
            };
            let (vertical_bar, horizontal_bar) = match shape {
                CursorShapeCase::Block => (false, false),
                CursorShapeCase::VerticalBar => (true, false),
                CursorShapeCase::HorizontalBar => (false, true),
            };
            let corners =
                corners_for_cursor(position.row, position.col, vertical_bar, horizontal_bar);

            CursorRectFixture {
                position,
                shape,
                corners,
            }
        })
        .boxed()
}

pub(crate) fn timer_id() -> BoxedStrategy<TimerId> {
    prop_oneof![
        Just(TimerId::Animation),
        Just(TimerId::Ingress),
        Just(TimerId::Recovery),
        Just(TimerId::Cleanup),
    ]
    .boxed()
}

pub(crate) fn timer_token() -> BoxedStrategy<TimerToken> {
    (timer_id(), any::<u64>())
        .prop_map(|(timer_id, generation)| {
            TimerToken::new(timer_id, TimerGeneration::new(generation))
        })
        .boxed()
}

pub(crate) fn window_placement() -> BoxedStrategy<WindowPlacement> {
    (
        -WINDOW_DIMENSION_LIMIT..=WINDOW_DIMENSION_LIMIT,
        -WINDOW_DIMENSION_LIMIT..=WINDOW_DIMENSION_LIMIT,
        1_u32..=WINDOW_PLACEMENT_WIDTH_LIMIT,
        50_u32..=350_u32,
    )
        .prop_map(|(row, col, width, zindex)| WindowPlacement {
            row,
            col,
            width,
            zindex,
        })
        .boxed()
}

pub(crate) fn conceal_window_state() -> BoxedStrategy<ConcealWindowState> {
    (
        -1_i64..=4_i64,
        vec(
            prop_oneof![
                Just('n'),
                Just('i'),
                Just('v'),
                Just('c'),
                Just('x'),
                Just('y'),
            ],
            0..=4,
        ),
    )
        .prop_map(|(conceallevel, concealcursor)| {
            ConcealWindowState::new(conceallevel, concealcursor.into_iter().collect::<String>())
        })
        .boxed()
}

pub(crate) fn cache_key_mutation_axis(axis_count: usize) -> BoxedStrategy<CacheKeyMutationAxis> {
    (0..axis_count).prop_map(CacheKeyMutationAxis).boxed()
}

pub(crate) fn window_pool_fixture(max_windows: usize) -> BoxedStrategy<WindowPoolFixture> {
    (
        0_usize..=max_windows,
        vec(
            (
                0_i64..80_i64,
                0_i64..240_i64,
                1_u16..16_u16,
                50_u32..350_u32,
                any::<u64>(),
                any::<bool>(),
            ),
            0..=max_windows,
        ),
    )
        .prop_map(|(expected_demand, entries)| {
            let windows = entries
                .into_iter()
                .enumerate()
                .map(
                    |(index, (row, col, width, zindex, last_used_epoch, visible))| {
                        let offset = i32::try_from(index).unwrap_or(i32::MAX);
                        let placement = WindowPoolPlacementSpec::builder()
                            .origin(row, col)
                            .width(width)
                            .zindex(zindex)
                            .build();
                        WindowPoolWindowSpec::builder()
                            .ids(
                                100_i32.saturating_add(offset),
                                200_i32.saturating_add(offset),
                            )
                            .last_used_epoch(last_used_epoch)
                            .visible(visible)
                            .placement(placement)
                            .build()
                    },
                )
                .collect();

            WindowPoolFixture::new(expected_demand, windows)
        })
        .boxed()
}

pub(crate) fn staged_render_step_samples(max_steps: usize) -> BoxedStrategy<Vec<RenderStepSample>> {
    vec((cursor_rectangle(), 0.0_f64..40.0_f64), 1..=max_steps)
        .prop_map(|samples| {
            samples
                .into_iter()
                .map(|(fixture, dt_ms)| RenderStepSample::new(fixture.corners, dt_ms))
                .collect()
        })
        .boxed()
}
