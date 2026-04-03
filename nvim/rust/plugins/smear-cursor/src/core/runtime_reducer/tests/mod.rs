use super::CleanupDirective;
use super::CleanupPolicyInput;
use super::CursorEventContext;
use super::CursorTransition;
use super::CursorVisibilityEffect;
use super::EventSource;
use super::MotionClass;
use super::RenderAction;
use super::RenderAllocationPolicy;
use super::RenderCleanupAction;
use super::RenderSideEffects;
use super::ScrollShift;
use super::TargetCellPresentation;
use super::decide_cleanup_directive;
use super::keep_warm_until_ms;
use super::next_cleanup_check_delay_ms;
use super::reduce_cursor_event;
use crate::config::RuntimeConfig;
use crate::core::state::SemanticEvent;
use crate::state::CursorLocation;
use crate::state::CursorShape;
use crate::state::RuntimeState;
use crate::types::Particle;
use crate::types::Point;
use crate::types::RenderFrame;
use crate::types::RenderStepSample;
use crate::types::StepOutput;
use proptest::collection::vec;
use proptest::prelude::*;
use std::fmt;
use std::sync::Arc;

fn render_action(transition: &CursorTransition) -> &RenderAction {
    &transition.render_decision.render_action
}

fn render_cleanup_action(transition: &CursorTransition) -> RenderCleanupAction {
    transition.render_decision.render_cleanup_action
}

fn render_allocation_policy(transition: &CursorTransition) -> RenderAllocationPolicy {
    transition.render_decision.render_allocation_policy
}

fn render_side_effects(transition: &CursorTransition) -> RenderSideEffects {
    transition.render_decision.render_side_effects
}

fn draw_frame(transition: &CursorTransition) -> Option<&RenderFrame> {
    match render_action(transition) {
        RenderAction::Draw(frame) => Some(frame.as_ref()),
        RenderAction::ClearAll | RenderAction::Noop => None,
    }
}

fn event(row: f64, col: f64) -> CursorEventContext {
    event_at(row, col, 100.0)
}

fn event_at(row: f64, col: f64, now_ms: f64) -> CursorEventContext {
    CursorEventContext {
        row,
        col,
        now_ms,
        seed: 7,
        cursor_location: CursorLocation::new(10, 20, 1, 1),
        scroll_shift: None,
        semantic_event: SemanticEvent::FrameCommitted,
    }
}

fn event_with_location(
    row: f64,
    col: f64,
    now_ms: f64,
    seed: u32,
    window_handle: i64,
    buffer_handle: i64,
) -> CursorEventContext {
    CursorEventContext {
        row,
        col,
        now_ms,
        seed,
        cursor_location: CursorLocation::new(window_handle, buffer_handle, 1, 1),
        scroll_shift: None,
        semantic_event: SemanticEvent::FrameCommitted,
    }
}

#[derive(Clone)]
struct TrajectoryStep {
    source: EventSource,
    event: CursorEventContext,
}

fn event_with_location_and_scroll(
    row: f64,
    col: f64,
    now_ms: f64,
    seed: u32,
    window_handle: i64,
    buffer_handle: i64,
    scroll_shift: Option<ScrollShift>,
) -> CursorEventContext {
    CursorEventContext {
        row,
        col,
        now_ms,
        seed,
        cursor_location: CursorLocation::new(window_handle, buffer_handle, 1, 1),
        scroll_shift,
        semantic_event: SemanticEvent::FrameCommitted,
    }
}

fn text_mutation_event(row: f64, col: f64, now_ms: f64) -> CursorEventContext {
    CursorEventContext {
        semantic_event: SemanticEvent::TextMutatedAtCursorContext,
        ..event_at(row, col, now_ms)
    }
}

fn external_step(
    row: f64,
    col: f64,
    now_ms: f64,
    seed: u32,
    window_handle: i64,
    buffer_handle: i64,
) -> TrajectoryStep {
    TrajectoryStep {
        source: EventSource::External,
        event: event_with_location(row, col, now_ms, seed, window_handle, buffer_handle),
    }
}

fn animation_tick_step(
    row: f64,
    col: f64,
    now_ms: f64,
    seed: u32,
    window_handle: i64,
    buffer_handle: i64,
) -> TrajectoryStep {
    TrajectoryStep {
        source: EventSource::AnimationTick,
        event: event_with_location(row, col, now_ms, seed, window_handle, buffer_handle),
    }
}

fn external_with_scroll_step(
    row: f64,
    col: f64,
    now_ms: f64,
    seed: u32,
    window_handle: i64,
    buffer_handle: i64,
    scroll_shift: ScrollShift,
) -> TrajectoryStep {
    TrajectoryStep {
        source: EventSource::External,
        event: event_with_location_and_scroll(
            row,
            col,
            now_ms,
            seed,
            window_handle,
            buffer_handle,
            Some(scroll_shift),
        ),
    }
}

fn animation_tick_with_scroll_step(
    row: f64,
    col: f64,
    now_ms: f64,
    seed: u32,
    window_handle: i64,
    buffer_handle: i64,
    scroll_shift: ScrollShift,
) -> TrajectoryStep {
    TrajectoryStep {
        source: EventSource::AnimationTick,
        event: event_with_location_and_scroll(
            row,
            col,
            now_ms,
            seed,
            window_handle,
            buffer_handle,
            Some(scroll_shift),
        ),
    }
}

fn corners_center(corners: &[Point; 4]) -> Point {
    Point {
        row: (corners[0].row + corners[1].row + corners[2].row + corners[3].row) / 4.0,
        col: (corners[0].col + corners[1].col + corners[2].col + corners[3].col) / 4.0,
    }
}

fn trajectory_center(state: &RuntimeState) -> Point {
    corners_center(&state.current_corners())
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum RoundedScalar {
    Finite(i64),
    PosInf,
    NegInf,
    NaN,
}

impl RoundedScalar {
    fn from_f64(value: f64) -> Self {
        if value.is_nan() {
            Self::NaN
        } else if value.is_infinite() {
            if value.is_sign_negative() {
                Self::NegInf
            } else {
                Self::PosInf
            }
        } else {
            Self::Finite((value * 1_000.0).round() as i64)
        }
    }
}

impl fmt::Debug for RoundedScalar {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Finite(value) => {
                let sign = if *value < 0 { "-" } else { "" };
                let abs = value.abs();
                write!(formatter, "{sign}{}.{:03}", abs / 1_000, abs % 1_000)
            }
            Self::PosInf => formatter.write_str("inf"),
            Self::NegInf => formatter.write_str("-inf"),
            Self::NaN => formatter.write_str("NaN"),
        }
    }
}

impl RoundedScalar {
    fn render(self) -> String {
        format!("{self:?}")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PointSummary {
    row: RoundedScalar,
    col: RoundedScalar,
}

impl PointSummary {
    fn from_point(point: Point) -> Self {
        Self {
            row: RoundedScalar::from_f64(point.row),
            col: RoundedScalar::from_f64(point.col),
        }
    }

    fn render(self) -> String {
        format!("{},{}", self.row.render(), self.col.render())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ScrollShiftSummary {
    row_shift: RoundedScalar,
    col_shift: RoundedScalar,
    min_row: RoundedScalar,
    max_row: RoundedScalar,
}

impl ScrollShiftSummary {
    fn from_scroll_shift(scroll_shift: ScrollShift) -> Self {
        Self {
            row_shift: RoundedScalar::from_f64(scroll_shift.row_shift),
            col_shift: RoundedScalar::from_f64(scroll_shift.col_shift),
            min_row: RoundedScalar::from_f64(scroll_shift.min_row),
            max_row: RoundedScalar::from_f64(scroll_shift.max_row),
        }
    }

    fn render(self) -> String {
        if self.col_shift == RoundedScalar::Finite(0) {
            return format!(
                "{}[{}..{}]",
                self.row_shift.render(),
                self.min_row.render(),
                self.max_row.render()
            );
        }

        format!(
            "{},{}[{}..{}]",
            self.row_shift.render(),
            self.col_shift.render(),
            self.min_row.render(),
            self.max_row.render()
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LocationSummary {
    window_handle: i64,
    buffer_handle: i64,
    top_row: i64,
    line: i64,
}

impl LocationSummary {
    fn from_location(location: &CursorLocation) -> Self {
        Self {
            window_handle: location.window_handle,
            buffer_handle: location.buffer_handle,
            top_row: location.top_row,
            line: location.line,
        }
    }

    fn render_surface(&self) -> String {
        format!("{}/{}", self.window_handle, self.buffer_handle)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EventSummary {
    now_ms: RoundedScalar,
    position: PointSummary,
    seed: u32,
    location: LocationSummary,
    scroll_shift: Option<ScrollShiftSummary>,
    semantic_event: SemanticEvent,
}

impl EventSummary {
    fn from_event(event: &CursorEventContext) -> Self {
        Self {
            now_ms: RoundedScalar::from_f64(event.now_ms),
            position: PointSummary::from_point(Point {
                row: event.row,
                col: event.col,
            }),
            seed: event.seed,
            location: LocationSummary::from_location(&event.cursor_location),
            scroll_shift: event
                .scroll_shift
                .map(ScrollShiftSummary::from_scroll_shift),
            semantic_event: event.semantic_event,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StepSampleSummary {
    center: PointSummary,
    dt_ms: RoundedScalar,
}

impl StepSampleSummary {
    fn from_step_sample(step_sample: &RenderStepSample) -> Self {
        Self {
            center: PointSummary::from_point(corners_center(&step_sample.corners)),
            dt_ms: RoundedScalar::from_f64(step_sample.dt_ms),
        }
    }

    fn render(&self) -> String {
        format!("{}@{}", self.center.render(), self.dt_ms.render())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RenderFrameSummary {
    center: PointSummary,
    target: PointSummary,
    planner_idle_steps: u32,
    vertical_bar: bool,
    trail_stroke_id: u64,
    retarget_epoch: u64,
    step_samples: Vec<StepSampleSummary>,
    particles: usize,
    color_at_cursor: Option<u32>,
}

impl RenderFrameSummary {
    fn from_render_frame(frame: &RenderFrame) -> Self {
        Self {
            center: PointSummary::from_point(corners_center(&frame.corners)),
            target: PointSummary::from_point(frame.target),
            planner_idle_steps: frame.planner_idle_steps,
            vertical_bar: frame.vertical_bar,
            trail_stroke_id: frame.trail_stroke_id.value(),
            retarget_epoch: frame.retarget_epoch,
            step_samples: frame
                .step_samples
                .iter()
                .map(StepSampleSummary::from_step_sample)
                .collect(),
            particles: frame.particle_count,
            color_at_cursor: frame.color_at_cursor,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RenderActionSummary {
    Draw(RenderFrameSummary),
    ClearAll,
    Noop,
}

impl RenderActionSummary {
    fn from_render_action(render_action: &RenderAction) -> Self {
        match render_action {
            RenderAction::Draw(frame) => Self::Draw(RenderFrameSummary::from_render_frame(frame)),
            RenderAction::ClearAll => Self::ClearAll,
            RenderAction::Noop => Self::Noop,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TransitionSummary {
    motion_class: MotionClass,
    should_schedule_next_animation: bool,
    next_animation_at_ms: Option<u64>,
    render_cleanup_action: RenderCleanupAction,
    render_allocation_policy: RenderAllocationPolicy,
    render_side_effects: RenderSideEffects,
    render_action: RenderActionSummary,
}

impl TransitionSummary {
    fn from_transition(transition: &CursorTransition) -> Self {
        Self {
            motion_class: transition.motion_class,
            should_schedule_next_animation: transition.should_schedule_next_animation,
            next_animation_at_ms: transition.next_animation_at_ms,
            render_cleanup_action: render_cleanup_action(transition),
            render_allocation_policy: render_allocation_policy(transition),
            render_side_effects: render_side_effects(transition),
            render_action: RenderActionSummary::from_render_action(render_action(transition)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RuntimeStateSummary {
    center: PointSummary,
    target: PointSummary,
    tracked_location: Option<LocationSummary>,
    is_animating: bool,
    is_settling: bool,
    is_draining: bool,
    trail_stroke_id: u64,
    retarget_epoch: u64,
}

impl RuntimeStateSummary {
    fn from_state(state: &RuntimeState) -> Self {
        Self {
            center: PointSummary::from_point(trajectory_center(state)),
            target: PointSummary::from_point(state.target_position()),
            tracked_location: state
                .tracked_location_ref()
                .map(LocationSummary::from_location),
            is_animating: state.is_animating(),
            is_settling: state.is_settling(),
            is_draining: state.is_draining(),
            trail_stroke_id: state.trail_stroke_id().value(),
            retarget_epoch: state.retarget_epoch(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TrajectoryRecord {
    source: EventSource,
    event: EventSummary,
    transition: TransitionSummary,
    state: RuntimeStateSummary,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TrajectoryTranscript {
    steps: Vec<TrajectoryRecord>,
    final_state: RuntimeStateSummary,
}

fn render_source(source: EventSource) -> &'static str {
    match source {
        EventSource::External => "ext",
        EventSource::AnimationTick => "tick",
    }
}

fn render_motion_class(motion_class: MotionClass) -> &'static str {
    match motion_class {
        MotionClass::Continuous => "cont",
        MotionClass::DiscontinuousJump => "jump",
        MotionClass::SurfaceRetarget => "surface",
    }
}

fn render_cleanup_action_token(action: RenderCleanupAction) -> &'static str {
    match action {
        RenderCleanupAction::NoAction => "none",
        RenderCleanupAction::Schedule => "schedule",
        RenderCleanupAction::Invalidate => "invalidate",
    }
}

fn render_allocation_policy_token(policy: RenderAllocationPolicy) -> &'static str {
    match policy {
        RenderAllocationPolicy::ReuseOnly => "reuse",
        RenderAllocationPolicy::BootstrapIfPoolEmpty => "bootstrap",
    }
}

fn render_cursor_visibility(effect: CursorVisibilityEffect) -> &'static str {
    match effect {
        CursorVisibilityEffect::Keep => "keep",
        CursorVisibilityEffect::Hide => "hide",
        CursorVisibilityEffect::Show => "show",
    }
}

fn render_target_cell_presentation(presentation: TargetCellPresentation) -> &'static str {
    match presentation {
        TargetCellPresentation::None => "none",
        TargetCellPresentation::OverlayCursorCell(crate::types::CursorCellShape::Block) => {
            "overlay"
        }
        TargetCellPresentation::OverlayCursorCell(crate::types::CursorCellShape::VerticalBar) => {
            "overlay_vbar"
        }
        TargetCellPresentation::OverlayCursorCell(crate::types::CursorCellShape::HorizontalBar) => {
            "overlay_hbar"
        }
    }
}

impl RenderActionSummary {
    fn render(&self) -> String {
        match self {
            Self::Draw(frame) => {
                let mut details = vec!["draw".to_string()];
                if !frame.step_samples.is_empty() {
                    details.push(format!(
                        "samples=[{}]",
                        frame
                            .step_samples
                            .iter()
                            .map(StepSampleSummary::render)
                            .collect::<Vec<_>>()
                            .join(";")
                    ));
                }
                if frame.planner_idle_steps > 0 {
                    details.push(format!("planner_idle={}", frame.planner_idle_steps));
                }
                if frame.vertical_bar {
                    details.push("shape=vbar".to_string());
                }
                if frame.particles > 0 {
                    details.push(format!("particles={}", frame.particles));
                }
                if let Some(color_at_cursor) = frame.color_at_cursor {
                    details.push(format!("cursor_color={color_at_cursor}"));
                }
                details.join(" ")
            }
            Self::ClearAll => "clear_all".to_string(),
            Self::Noop => "noop".to_string(),
        }
    }
}

impl TrajectoryRecord {
    fn render(&self, index: usize) -> String {
        let mut fields = vec![
            format!("{index:02}"),
            render_source(self.source).to_string(),
            format!("t={}", self.event.now_ms.render()),
            format!("pos={}", self.event.position.render()),
            format!("surf={}", self.event.location.render_surface()),
        ];
        if let Some(scroll_shift) = self.event.scroll_shift {
            fields.push(format!("scroll={}", scroll_shift.render()));
        }
        if self.event.semantic_event != SemanticEvent::FrameCommitted {
            fields.push(format!("semantic={:?}", self.event.semantic_event));
        }
        fields.push(format!(
            "motion={}",
            render_motion_class(self.transition.motion_class)
        ));
        fields.push(format!("action={}", self.transition.render_action.render()));
        fields.push(format!("center={}", self.state.center.render()));
        fields.push(format!("target={}", self.state.target.render()));
        fields.push(format!(
            "next={}",
            self.transition
                .next_animation_at_ms
                .map_or_else(|| "-".to_string(), |deadline| deadline.to_string())
        ));
        fields.push(format!(
            "cleanup={}",
            render_cleanup_action_token(self.transition.render_cleanup_action)
        ));
        fields.push(format!(
            "alloc={}",
            render_allocation_policy_token(self.transition.render_allocation_policy)
        ));
        fields.push(format!(
            "vis={}",
            render_cursor_visibility(self.transition.render_side_effects.cursor_visibility)
        ));
        if self
            .transition
            .render_side_effects
            .redraw_after_draw_if_cmdline
        {
            fields.push("cmdline_draw_redraw=1".to_string());
        }
        if self
            .transition
            .render_side_effects
            .redraw_after_clear_if_cmdline
        {
            fields.push("cmdline_clear_redraw=1".to_string());
        }
        if self.transition.render_side_effects.target_cell_presentation
            != TargetCellPresentation::None
        {
            fields.push(format!(
                "target_cell={}",
                render_target_cell_presentation(
                    self.transition.render_side_effects.target_cell_presentation
                )
            ));
        }
        if !self
            .transition
            .render_side_effects
            .allow_real_cursor_updates
        {
            fields.push("real_cursor=off".to_string());
        }
        fields.push(format!(
            "sched={}",
            if self.transition.should_schedule_next_animation {
                1
            } else {
                0
            }
        ));
        fields.push(format!(
            "state=a{}s{}d{}",
            u8::from(self.state.is_animating),
            u8::from(self.state.is_settling),
            u8::from(self.state.is_draining)
        ));
        fields.push(format!("stroke={}", self.state.trail_stroke_id));
        fields.push(format!("epoch={}", self.state.retarget_epoch));
        fields.join(" ")
    }
}

impl TrajectoryTranscript {
    fn render(&self) -> String {
        let mut lines = self
            .steps
            .iter()
            .enumerate()
            .map(|(index, record)| record.render(index))
            .collect::<Vec<_>>();
        let mut final_line = vec![
            "final".to_string(),
            format!("center={}", self.final_state.center.render()),
            format!("target={}", self.final_state.target.render()),
            format!(
                "state=a{}s{}d{}",
                u8::from(self.final_state.is_animating),
                u8::from(self.final_state.is_settling),
                u8::from(self.final_state.is_draining)
            ),
            format!("stroke={}", self.final_state.trail_stroke_id),
            format!("epoch={}", self.final_state.retarget_epoch),
        ];
        if let Some(location) = &self.final_state.tracked_location {
            final_line.push(format!("surf={}", location.render_surface()));
        }
        lines.push(final_line.join(" "));
        lines.join("\n")
    }
}

fn trajectory_transcript(
    state: &mut RuntimeState,
    mode: &str,
    steps: &[TrajectoryStep],
) -> TrajectoryTranscript {
    let mut records = Vec::with_capacity(steps.len());

    for step in steps {
        let transition = reduce_cursor_event(state, mode, step.event.clone(), step.source);
        records.push(TrajectoryRecord {
            source: step.source,
            event: EventSummary::from_event(&step.event),
            transition: TransitionSummary::from_transition(&transition),
            state: RuntimeStateSummary::from_state(state),
        });
    }

    TrajectoryTranscript {
        steps: records,
        final_state: RuntimeStateSummary::from_state(state),
    }
}

fn trajectory_transcript_with_fresh_state(
    seed_state: &RuntimeState,
    mode: &str,
    steps: &[TrajectoryStep],
) -> TrajectoryTranscript {
    let mut replay = seed_state.clone();
    trajectory_transcript(&mut replay, mode, steps)
}

fn initialized_runtime(
    mode: &str,
    configure: impl FnOnce(&mut RuntimeState),
) -> (RuntimeState, CursorTransition) {
    let mut state = RuntimeState::default();
    configure(&mut state);
    let transition = reduce_cursor_event(&mut state, mode, event(5.0, 6.0), EventSource::External);
    (state, transition)
}

fn delayed_retarget_scenario() -> (RuntimeState, CursorTransition) {
    let (mut state, _) = initialized_runtime("n", |state| {
        state.config.delay_event_to_smear = 40.0;
    });
    let pending = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, 16.0, 116.0),
        EventSource::External,
    );
    (state, pending)
}

fn animating_runtime_after_kickoff(
    configure: impl FnOnce(&mut RuntimeState),
) -> (RuntimeState, CursorTransition) {
    let (mut state, _) = initialized_runtime("n", configure);
    let kickoff = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, 12.0, 116.0),
        EventSource::External,
    );
    (state, kickoff)
}

fn animating_runtime_towards_target(configure: impl FnOnce(&mut RuntimeState)) -> RuntimeState {
    let mut state = RuntimeState::default();
    configure(&mut state);
    state.initialize_cursor(
        Point { row: 5.0, col: 6.0 },
        CursorShape::new(false, false),
        7,
        &CursorLocation::new(10, 20, 1, 1),
    );
    state.set_target(
        Point {
            row: 5.0,
            col: 12.0,
        },
        CursorShape::new(false, false),
    );
    state.start_animation_towards_target();
    state.set_last_tick_ms(Some(100.0));
    state
}

fn advance_until_tail_drain(state: &mut RuntimeState) -> (u32, CursorTransition) {
    // Surprising: stateful springs make settle timing highly trajectory-dependent, so callers
    // should assert lifecycle invariants here rather than exact frame counts.
    for tick in 1_u32..=160_u32 {
        let now_ms = 100.0 + 16.0 * f64::from(tick);
        let transition = reduce_cursor_event(
            state,
            "n",
            event_at(5.0, 12.0, now_ms),
            EventSource::AnimationTick,
        );
        if state.is_draining() {
            return (tick, transition);
        }
    }

    panic!("animation should eventually settle and start draining");
}

mod bootstrap_and_frame_building;
mod cleanup_policy;
mod cursor_visibility_side_effects;
mod delayed_settling_transitions;
mod fixed_step_stability;
mod jump_classification;
mod mode_specific_transitions;
mod property_invariants;
mod render_frame_caching;
mod retargeting_while_animating;
mod tail_drain_lifecycle;
mod trajectory_goldens;
mod viewport_scroll_translation;
mod window_and_buffer_jump_policies;
mod window_resize_reflow;
