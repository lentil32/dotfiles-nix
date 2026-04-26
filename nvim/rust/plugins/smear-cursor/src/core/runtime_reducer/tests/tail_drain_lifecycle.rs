use super::*;
use crate::core::runtime_reducer::as_delay_ms;
use crate::core::types::AnimationSchedule;
use crate::core::types::Millis;
use crate::test_support::proptest::stateful_config;
use pretty_assertions::assert_eq;
use proptest::collection::vec;

const CLOCK_DISCONTINUITY_CATCH_UP_WINDOWS: f64 = 8.0;

#[derive(Clone, Debug)]
struct TailDrainCountdownCase {
    remaining_steps: u32,
    max_simulation_steps_per_frame: u32,
    frame_period_ms: u16,
    simulation_hz: u16,
    tick_gaps_ms: Vec<u16>,
}

impl TailDrainCountdownCase {
    fn completion_gap_ms(&self) -> f64 {
        let simulation_step_ms = (1000.0 / f64::from(self.simulation_hz)).ceil().max(1.0);
        f64::from(self.frame_period_ms).max(simulation_step_ms)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TailDrainExpectedAction {
    Draw { planner_idle_steps: u32 },
    Noop,
    ClearAll,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TailDrainExpectedTransition {
    action: TailDrainExpectedAction,
    animation_schedule: AnimationSchedule,
    render_cleanup_action: RenderCleanupAction,
    remaining_steps: u32,
}

#[derive(Clone, Copy, Debug)]
struct TailDrainModel {
    remaining_steps: u32,
    last_tick_ms: Option<f64>,
    simulation_accumulator_ms: f64,
    next_frame_at_ms: Option<f64>,
}

impl TailDrainModel {
    fn new(remaining_steps: u32, last_tick_ms: Option<f64>) -> Self {
        Self {
            remaining_steps,
            last_tick_ms,
            simulation_accumulator_ms: 0.0,
            next_frame_at_ms: None,
        }
    }

    fn step(
        &mut self,
        config: &crate::config::RuntimeConfig,
        now_ms: f64,
    ) -> TailDrainExpectedTransition {
        let frame_period_ms = config.time_interval.max(1.0);
        let elapsed_ms = self
            .last_tick_ms
            .map_or(frame_period_ms, |previous| (now_ms - previous).max(0.0));
        let simulation_step_ms = config.simulation_step_interval_ms().max(1.0);
        let catch_up_budget_ms = frame_period_ms
            .max(simulation_step_ms * f64::from(config.max_simulation_steps_per_frame.max(1)))
            * CLOCK_DISCONTINUITY_CATCH_UP_WINDOWS;

        if self.last_tick_ms.is_some() && elapsed_ms > catch_up_budget_ms {
            self.last_tick_ms = None;
            self.simulation_accumulator_ms = 0.0;
            self.next_frame_at_ms = None;
            self.remaining_steps = 0;
            return TailDrainExpectedTransition {
                action: TailDrainExpectedAction::ClearAll,
                animation_schedule: AnimationSchedule::Idle,
                render_cleanup_action: RenderCleanupAction::Schedule,
                remaining_steps: 0,
            };
        }

        self.last_tick_ms = Some(now_ms);
        self.simulation_accumulator_ms += elapsed_ms;

        let max_simulation_steps =
            usize::try_from(config.max_simulation_steps_per_frame).unwrap_or(usize::MAX);
        let mut planner_idle_steps = 0_u32;
        let mut executed_steps = 0_usize;

        while executed_steps < max_simulation_steps
            && self.remaining_steps > 0
            && self.simulation_accumulator_ms >= simulation_step_ms
        {
            self.simulation_accumulator_ms -= simulation_step_ms;
            self.remaining_steps -= 1;
            planner_idle_steps = planner_idle_steps.saturating_add(1);
            executed_steps = executed_steps.saturating_add(1);
        }

        if self.remaining_steps == 0 {
            self.next_frame_at_ms = None;
            return TailDrainExpectedTransition {
                action: TailDrainExpectedAction::ClearAll,
                animation_schedule: AnimationSchedule::Idle,
                render_cleanup_action: RenderCleanupAction::Schedule,
                remaining_steps: 0,
            };
        }

        let animation_schedule = AnimationSchedule::Deadline(Millis::new(
            self.next_animation_deadline(now_ms, frame_period_ms),
        ));
        if planner_idle_steps == 0 {
            return TailDrainExpectedTransition {
                action: TailDrainExpectedAction::Noop,
                animation_schedule,
                render_cleanup_action: RenderCleanupAction::Invalidate,
                remaining_steps: self.remaining_steps,
            };
        }

        TailDrainExpectedTransition {
            action: TailDrainExpectedAction::Draw { planner_idle_steps },
            animation_schedule,
            render_cleanup_action: RenderCleanupAction::Invalidate,
            remaining_steps: self.remaining_steps,
        }
    }

    fn next_animation_deadline(&mut self, now_ms: f64, frame_period_ms: f64) -> u64 {
        let mut next_frame_at_ms = self.next_frame_at_ms.unwrap_or(now_ms + frame_period_ms);
        if next_frame_at_ms <= now_ms {
            let missed_frames = ((now_ms - next_frame_at_ms) / frame_period_ms).floor() + 1.0;
            next_frame_at_ms += missed_frames * frame_period_ms;
        }
        self.next_frame_at_ms = Some(next_frame_at_ms);
        as_delay_ms(next_frame_at_ms.max(now_ms + 1.0))
    }
}

fn tail_drain_countdown_case() -> impl Strategy<Value = TailDrainCountdownCase> {
    (
        1_u32..=24_u32,
        1_u32..=6_u32,
        1_u16..=24_u16,
        24_u16..=240_u16,
        vec(0_u16..=40_u16, 0..=8),
    )
        .prop_map(
            |(
                remaining_steps,
                max_simulation_steps_per_frame,
                frame_period_ms,
                simulation_hz,
                tick_gaps_ms,
            )| TailDrainCountdownCase {
                remaining_steps,
                max_simulation_steps_per_frame,
                frame_period_ms,
                simulation_hz,
                tick_gaps_ms,
            },
        )
}

fn draining_runtime(case: &TailDrainCountdownCase) -> RuntimeState {
    let mut state = RuntimeState::default();
    state.config.time_interval = f64::from(case.frame_period_ms);
    state.config.simulation_hz = f64::from(case.simulation_hz);
    state.config.max_simulation_steps_per_frame = case.max_simulation_steps_per_frame;
    state.initialize_cursor(
        RenderPoint { row: 5.0, col: 6.0 },
        CursorShape::block(),
        7,
        &TrackedCursor::fixture(10, 20, 1, 1),
    );
    state.start_tail_drain(case.remaining_steps, 100.0);
    state
}

fn assert_tail_drain_transition_matches_model(
    transition: &CursorTransition,
    state: &RuntimeState,
    expected: TailDrainExpectedTransition,
) {
    match expected.action {
        TailDrainExpectedAction::Draw { planner_idle_steps } => {
            let frame = draw_frame(transition).expect("expected drain tick to draw");
            assert_eq!(frame.planner_idle_steps, planner_idle_steps);
        }
        TailDrainExpectedAction::Noop => {
            assert!(matches!(render_action(transition), RenderAction::Noop));
        }
        TailDrainExpectedAction::ClearAll => {
            assert!(matches!(render_action(transition), RenderAction::ClearAll));
        }
    }

    assert_eq!(transition.animation_schedule, expected.animation_schedule);
    assert_eq!(
        render_cleanup_action(transition),
        expected.render_cleanup_action
    );
    assert_eq!(state.is_draining(), expected.remaining_steps > 0);
    assert_eq!(state.drain_steps_remaining(), expected.remaining_steps);
    assert!(!state.is_animating());
}

#[test]
fn quiescent_external_noop_after_first_frame_schedules_cleanup() {
    let (mut state, _) = initialized_runtime("n", |_| {});
    let follow_up = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, 6.0, 116.0),
        EventSource::External,
    );

    assert!(matches!(render_action(&follow_up), RenderAction::Noop));
    assert!(!follow_up.should_schedule_next_animation());
    assert_eq!(
        render_side_effects(&follow_up).cursor_visibility,
        CursorVisibilityEffect::Show
    );
    assert_eq!(
        render_cleanup_action(&follow_up),
        RenderCleanupAction::Schedule
    );
    assert!(!state.is_animating());
    assert!(!state.is_draining());
}

#[test]
fn tail_drain_gap_beyond_catch_up_budget_clears_tail_immediately() {
    let case = TailDrainCountdownCase {
        remaining_steps: 2,
        max_simulation_steps_per_frame: 1,
        frame_period_ms: 1,
        simulation_hz: 217,
        tick_gaps_ms: vec![37],
    };
    let mut state = draining_runtime(&case);

    let transition = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, 6.0, 137.0),
        EventSource::AnimationTick,
    );

    assert!(matches!(render_action(&transition), RenderAction::ClearAll));
    assert!(!transition.should_schedule_next_animation());
    assert_eq!(transition.next_animation_at_ms(), None);
    assert_eq!(
        render_cleanup_action(&transition),
        RenderCleanupAction::Schedule
    );
    assert_eq!(state.last_tick_ms(), None);
    assert!(!state.is_draining());
    assert_eq!(state.drain_steps_remaining(), 0);
}

proptest! {
    #![proptest_config(stateful_config())]

    #[test]
    fn prop_tail_drain_entry_respects_hold_threshold_and_starts_with_draw_frame(
        stop_hold_frames in 1_u32..8_u32,
    ) {
        let mut state = animating_runtime_towards_target(|state| {
            state.config.stop_distance_enter = 10.0;
            state.config.stop_distance_exit = 10.0;
            state.config.stop_velocity_enter = 10.0;
            state.config.stop_hold_frames = stop_hold_frames;
        });

        let (drained_at_tick, transition) = advance_until_tail_drain(&mut state);

        prop_assert!(
            drained_at_tick >= stop_hold_frames,
            "tail drain must not start before stop_hold_frames consecutive enter probes",
        );
        let frame = draw_frame(&transition).expect("tail drain entry should draw");
        prop_assert_eq!(frame.planner_idle_steps, 0);
        prop_assert!(transition.should_schedule_next_animation());
        prop_assert!(transition.next_animation_at_ms().is_some());
        prop_assert_eq!(
            render_cleanup_action(&transition),
            RenderCleanupAction::Invalidate,
        );
        prop_assert!(state.is_draining());
        prop_assert!(!state.is_animating());
        prop_assert!(state.drain_steps_remaining() > 0);
    }

    #[test]
    fn prop_tail_drain_animation_ticks_follow_the_reference_countdown_model(
        case in tail_drain_countdown_case(),
    ) {
        let mut state = draining_runtime(&case);
        let mut model = TailDrainModel::new(case.remaining_steps, Some(100.0));
        let mut now_ms = 100.0;

        for gap_ms in &case.tick_gaps_ms {
            now_ms += f64::from(*gap_ms);
            let expected = model.step(&state.config, now_ms);
            let transition = reduce_cursor_event(
                &mut state,
                "n",
                event_at(5.0, 6.0, now_ms),
                EventSource::AnimationTick,
            );
            assert_tail_drain_transition_matches_model(&transition, &state, expected);
            if !state.is_draining() {
                break;
            }
        }

        let completion_gap_ms = case.completion_gap_ms();
        for _ in 0..64_u32 {
            if !state.is_draining() {
                break;
            }
            now_ms += completion_gap_ms;
            let expected = model.step(&state.config, now_ms);
            let transition = reduce_cursor_event(
                &mut state,
                "n",
                event_at(5.0, 6.0, now_ms),
                EventSource::AnimationTick,
            );
            assert_tail_drain_transition_matches_model(&transition, &state, expected);
        }

        prop_assert!(!state.is_draining());
        prop_assert_eq!(state.drain_steps_remaining(), 0);

        let follow_up = reduce_cursor_event(
            &mut state,
            "n",
            event_at(5.0, 6.0, now_ms + 1.0),
            EventSource::External,
        );
        prop_assert!(matches!(render_action(&follow_up), RenderAction::Noop));
        prop_assert!(!follow_up.should_schedule_next_animation());
        prop_assert_eq!(follow_up.next_animation_at_ms(), None);
        prop_assert_eq!(
            render_side_effects(&follow_up).cursor_visibility,
            CursorVisibilityEffect::Show,
        );
        prop_assert_eq!(
            render_cleanup_action(&follow_up),
            RenderCleanupAction::Schedule,
        );
        prop_assert!(!state.is_animating());
        prop_assert!(!state.is_draining());
    }
}

#[test]
fn tail_drain_advances_existing_particles_without_emitting_new_ones() {
    let mut state = RuntimeState::default();
    state.config.particles_enabled = true;
    state.config.particles_per_second = 0.0;
    state.config.particles_per_length = 0.0;
    state.initialize_cursor(
        RenderPoint { row: 5.0, col: 6.0 },
        CursorShape::block(),
        7,
        &TrackedCursor::fixture(10, 20, 1, 1),
    );
    state.apply_step_output(StepOutput {
        current_corners: state.current_corners(),
        velocity_corners: state.velocity_corners(),
        spring_velocity_corners: state.spring_velocity_corners(),
        trail_elapsed_ms: state.trail_elapsed_ms(),
        particles: vec![Particle {
            position: RenderPoint { row: 5.0, col: 7.0 },
            velocity: RenderPoint { row: 2.0, col: 1.0 },
            lifetime: 500.0,
        }],
        previous_center: state.previous_center(),
        index_head: 0,
        index_tail: 0,
        rng_state: state.rng_state(),
    });
    state.start_tail_drain(4, 100.0);
    state.set_last_tick_ms(Some(100.0));

    let particles_before_drain_tick = state.particles().to_vec();
    let transition = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, 6.0, 116.0),
        EventSource::AnimationTick,
    );
    let frame = draw_frame(&transition).expect("drain tick should draw");
    assert!(frame.planner_idle_steps > 0);

    let particles_after_drain_tick = state.particles().to_vec();
    assert!(
        particles_after_drain_tick.len() <= particles_before_drain_tick.len(),
        "drain ticks must not emit new particles"
    );
    assert!(
        particles_after_drain_tick.len() != particles_before_drain_tick.len()
            || particles_after_drain_tick
                .iter()
                .zip(&particles_before_drain_tick)
                .any(|(after, before)| {
                    after.position != before.position
                        || after.velocity != before.velocity
                        || after.lifetime < before.lifetime
                }),
        "drain ticks should advance or retire existing particles"
    );
}
