use crate::animation::corners_for_cursor;
use crate::core::types::TimerId;
use crate::position::RenderPoint;
use crate::types::RenderStepSample;
use proptest::collection::vec;
use proptest::prelude::*;

const PURE_PROPTEST_CASES: u32 = 96;
const STATEFUL_PROPTEST_CASES: u32 = 48;
pub(crate) const DEFAULT_FLOAT_EPSILON: f64 = 1.0e-6;

const FINITE_POINT_LIMIT: f64 = 4096.0;

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
    mode: String,
}

impl ModeCase {
    pub(crate) fn new(mode: impl Into<String>) -> Self {
        Self { mode: mode.into() }
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
    pub(crate) position: RenderPoint,
    pub(crate) shape: CursorShapeCase,
    pub(crate) corners: [RenderPoint; 4],
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

pub(crate) fn approx_eq_point(left: RenderPoint, right: RenderPoint, epsilon: f64) -> bool {
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
        Just(ModeCase::new("n")),
        Just(ModeCase::new("no")),
        Just(ModeCase::new("i")),
        Just(ModeCase::new("ic")),
        Just(ModeCase::new("R")),
        Just(ModeCase::new("Rc")),
        Just(ModeCase::new("t")),
        Just(ModeCase::new("cv")),
        Just(ModeCase::new("v")),
    ]
    .boxed()
}

pub(crate) fn finite_point() -> BoxedStrategy<RenderPoint> {
    (
        -FINITE_POINT_LIMIT..FINITE_POINT_LIMIT,
        -FINITE_POINT_LIMIT..FINITE_POINT_LIMIT,
    )
        .prop_map(|(row, col)| RenderPoint { row, col })
        .boxed()
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
            let position = RenderPoint {
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
    proptest::sample::select(Vec::from(TimerId::ALL)).boxed()
}

pub(crate) fn cache_key_mutation_axis(axis_count: usize) -> BoxedStrategy<CacheKeyMutationAxis> {
    (0..axis_count).prop_map(CacheKeyMutationAxis).boxed()
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
