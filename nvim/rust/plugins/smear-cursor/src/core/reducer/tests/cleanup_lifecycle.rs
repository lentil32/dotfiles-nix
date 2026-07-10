use super::*;

fn assert_hot_cleanup_state(cleanup: RenderCleanupState, next_due_at: Millis, hard_due_at: Millis) {
    let RenderCleanupState::Hot(schedule) = cleanup else {
        panic!("expected hot cleanup state, got {cleanup:?}");
    };
    pretty_assert_eq!(schedule.next_compaction_due_at(), next_due_at);
    pretty_assert_eq!(schedule.hard_purge_due_at(), hard_due_at);
}

fn assert_cooling_cleanup_state(
    cleanup: RenderCleanupState,
    entered_cooling_at: Millis,
    next_due_at: Millis,
    hard_due_at: Millis,
) {
    let RenderCleanupState::Cooling(schedule) = cleanup else {
        panic!("expected cooling cleanup state, got {cleanup:?}");
    };
    pretty_assert_eq!(schedule.entered_cooling_at(), entered_cooling_at);
    pretty_assert_eq!(schedule.next_compaction_due_at(), next_due_at);
    pretty_assert_eq!(schedule.hard_purge_due_at(), hard_due_at);
}

fn assert_cold_cleanup_state(cleanup: RenderCleanupState) {
    pretty_assert_eq!(cleanup, RenderCleanupState::Cold);
}

#[test]
fn late_hot_cleanup_timer_soft_clears_before_hard_purge() {
    let mut runtime = ready_state_with_observation(cursor(/*row*/ 4, /*col*/ 9))
        .runtime()
        .clone();
    runtime.config.max_kept_windows = 21;
    let cleanup = RenderCleanupState::scheduled(
        Millis::new(/*value*/ 100),
        /*soft_delay_ms*/ 30,
        /*hard_delay_ms*/ 90,
    );
    let state = ready_state_with_observation(cursor(/*row*/ 4, /*col*/ 9))
        .with_runtime(runtime)
        .with_render_cleanup(cleanup);
    let (timers, token) = state.timers().arm(TimerId::Cleanup);
    let state = state.with_timers(timers);
    let expected_state = state
        .clone()
        .with_timers(state.timers().clear_matching(token));

    let transition = reduce(&state, cleanup_tick_event(token, /*observed_at*/ 10_000));

    pretty_assert_eq!(
        transition,
        Transition::new(
            expected_state,
            vec![Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
                execution: RenderCleanupExecution::SoftClear {
                    max_kept_windows: 21,
                },
            })],
        )
    );
}

#[test]
fn late_soft_clear_application_rebases_hard_purge_deadline_before_compaction() {
    let mut runtime = ready_state_with_observation(cursor(/*row*/ 4, /*col*/ 9))
        .runtime()
        .clone();
    runtime.config.max_kept_windows = 21;
    let hard_cleanup_delay_ms = render_hard_cleanup_delay_ms(&runtime.config);
    let target_budget =
        crate::core::runtime_reducer::render_cleanup_idle_target_budget(&runtime.config);
    let max_teardown_attempts_per_tick =
        crate::core::runtime_reducer::render_cleanup_max_teardown_attempts_per_tick(
            &runtime.config,
        );
    let cleanup = RenderCleanupState::scheduled(
        Millis::new(/*value*/ 100),
        /*soft_delay_ms*/ 30,
        /*hard_delay_ms*/ 90,
    );
    let observed_at = Millis::new(/*value*/ 10_000);
    let expected_cleanup =
        cleanup.enter_cooling_after_soft_clear(observed_at, hard_cleanup_delay_ms);
    let state = ready_state_with_observation(cursor(/*row*/ 4, /*col*/ 9))
        .with_runtime(runtime)
        .with_render_cleanup(cleanup);

    let transition = reduce(
        &state,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at,
            action: RenderCleanupAppliedAction::SoftCleared {
                retained_resources: 0,
            },
        }),
    );
    let actual = (
        transition.next.render_cleanup(),
        transition.next.timers().active_token(TimerId::Cleanup),
        transition.effects,
    );
    let expected = (
        expected_cleanup,
        None,
        vec![Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
            execution: RenderCleanupExecution::CompactToBudget {
                target_budget,
                max_teardown_attempts_per_tick,
            },
        })],
    );

    pretty_assert_eq!(actual, expected);
}

#[test]
fn late_soft_clear_while_cooling_restarts_bounded_compaction_before_hard_purge() {
    let runtime = ready_state_with_observation(cursor(/*row*/ 4, /*col*/ 9))
        .runtime()
        .clone();
    let hard_cleanup_delay_ms = render_hard_cleanup_delay_ms(&runtime.config);
    let cleanup = RenderCleanupState::scheduled(
        Millis::new(/*value*/ 100),
        /*soft_delay_ms*/ 30,
        /*hard_delay_ms*/ 90,
    )
    .enter_cooling(Millis::new(/*value*/ 130))
    .continue_cooling_after_progress(
        Millis::new(/*value*/ 160),
        /*cadence_delay_ms*/ 30,
        /*hard_cleanup_delay_ms*/ 90,
    );
    let observed_at = Millis::new(/*value*/ 10_000);
    let state = ready_state_with_observation(cursor(/*row*/ 4, /*col*/ 9))
        .with_runtime(runtime.clone())
        .with_render_cleanup(cleanup);

    let transition = reduce(
        &state,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at,
            action: RenderCleanupAppliedAction::SoftCleared {
                retained_resources: 0,
            },
        }),
    );

    assert_cooling_cleanup_state(
        transition.next.render_cleanup(),
        Millis::new(/*value*/ 130),
        observed_at,
        Millis::new(observed_at.value().saturating_add(hard_cleanup_delay_ms)),
    );
    pretty_assert_eq!(
        transition.effects,
        vec![Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
            execution: RenderCleanupExecution::CompactToBudget {
                target_budget: crate::core::runtime_reducer::render_cleanup_idle_target_budget(
                    &runtime.config,
                ),
                max_teardown_attempts_per_tick:
                    crate::core::runtime_reducer::render_cleanup_max_teardown_attempts_per_tick(
                        &runtime.config,
                    ),
            },
        })]
    );
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct RuntimeCleanupColdStorageResidency {
    preview_particles_scratch_retained: bool,
    render_step_samples_scratch_retained: bool,
    particle_aggregation_scratch_index_retained: bool,
    particle_aggregation_scratch_cells_retained: bool,
    particle_aggregation_scratch_screen_cells_retained: bool,
    cached_aggregated_particle_cells_retained: bool,
    cached_particle_screen_cells_retained: bool,
}

impl RuntimeCleanupColdStorageResidency {
    const RETAINED: Self = Self {
        preview_particles_scratch_retained: true,
        render_step_samples_scratch_retained: true,
        particle_aggregation_scratch_index_retained: true,
        particle_aggregation_scratch_cells_retained: true,
        particle_aggregation_scratch_screen_cells_retained: true,
        cached_aggregated_particle_cells_retained: true,
        cached_particle_screen_cells_retained: true,
    };

    const RELEASED: Self = Self {
        preview_particles_scratch_retained: false,
        render_step_samples_scratch_retained: false,
        particle_aggregation_scratch_index_retained: false,
        particle_aggregation_scratch_cells_retained: false,
        particle_aggregation_scratch_screen_cells_retained: false,
        cached_aggregated_particle_cells_retained: false,
        cached_particle_screen_cells_retained: false,
    };
}

fn runtime_cleanup_cold_storage_residency(
    runtime: &RuntimeState,
) -> RuntimeCleanupColdStorageResidency {
    RuntimeCleanupColdStorageResidency {
        preview_particles_scratch_retained: runtime.preview_particles_scratch_capacity() > 0,
        render_step_samples_scratch_retained: runtime.render_step_samples_scratch_capacity() > 0,
        particle_aggregation_scratch_index_retained: runtime
            .particle_aggregation_scratch_index_capacity()
            > 0,
        particle_aggregation_scratch_cells_retained: runtime
            .particle_aggregation_scratch_cells_capacity()
            > 0,
        particle_aggregation_scratch_screen_cells_retained: runtime
            .particle_aggregation_scratch_screen_cells_capacity()
            > 0,
        cached_aggregated_particle_cells_retained: runtime.has_cached_aggregated_particle_cells(),
        cached_particle_screen_cells_retained: runtime.has_cached_particle_screen_cells(),
    }
}

fn runtime_with_retained_cleanup_cold_storage(mut runtime: RuntimeState) -> RuntimeState {
    runtime.apply_step_output(crate::types::StepOutput {
        current_corners: [RenderPoint { row: 4.0, col: 5.0 }; 4],
        velocity_corners: [RenderPoint { row: 1.0, col: 2.0 }; 4],
        spring_velocity_corners: [RenderPoint {
            row: 0.25,
            col: 0.5,
        }; 4],
        trail_elapsed_ms: [8.0, 8.0, 8.0, 8.0],
        particles: vec![crate::types::Particle {
            position: RenderPoint { row: 6.0, col: 7.0 },
            velocity: RenderPoint {
                row: 0.5,
                col: 0.25,
            },
            lifetime: 0.75,
        }],
        previous_center: RenderPoint { row: 8.0, col: 9.0 },
        index_head: 0,
        index_tail: 3,
        rng_state: 1234,
    });
    let _ = runtime.shared_particle_screen_cells();
    runtime.reclaim_preview_particles_scratch(Vec::with_capacity(8));
    let mut scratch = runtime.take_render_step_samples_scratch();
    scratch.reserve(4);
    scratch.push(crate::types::RenderStepSample::new(
        [RenderPoint {
            row: 13.0,
            col: 14.0,
        }; 4],
        16.0,
    ));
    runtime.reclaim_render_step_samples_scratch(scratch);
    runtime
}

#[test]
fn cleanup_timer_soft_clear_immediately_emits_first_cooling_compaction() {
    let mut runtime = ready_state_with_observation(cursor(4, 9)).runtime().clone();
    runtime.config.max_kept_windows = 21;
    let state = ready_state_with_observation(cursor(4, 9)).with_runtime(runtime.clone());
    let basis = PatchBasis::new(None, None);
    let patch = ScenePatch::derive(basis);
    let (state, proposal_id) = state.allocate_proposal_id();
    let proposal = InFlightProposal::clear(
        proposal_id,
        patch,
        RealizationClear::new(21),
        RenderCleanupAction::Schedule,
        RenderSideEffects::default(),
        crate::core::state::AnimationSchedule::Idle,
    )
    .expect("clear proposal should be constructible");
    let staged = state
        .enter_planning(proposal_id)
        .expect("staging clear proposal requires a ready observation")
        .enter_applying(proposal)
        .expect("staging clear proposal requires the matching planning proposal");

    let completed = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(79),
            visual_change: true,
        }),
    );
    assert_hot_cleanup_state(
        completed.next.render_cleanup(),
        Millis::new(79 + render_cleanup_delay_ms(&runtime.config)),
        Millis::new(79 + render_hard_cleanup_delay_ms(&runtime.config)),
    );
    let soft_token = completed
        .next
        .timers()
        .active_token(TimerId::Cleanup)
        .expect("soft cleanup timer should be armed");

    let soft_tick = reduce(
        &completed.next,
        cleanup_tick_event(soft_token, 79 + render_cleanup_delay_ms(&runtime.config)),
    );

    pretty_assert_eq!(
        soft_tick.effects,
        vec![Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
            execution: RenderCleanupExecution::SoftClear {
                max_kept_windows: 21,
            },
        })]
    );

    let after_soft = reduce(
        &soft_tick.next,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at: Millis::new(79 + render_cleanup_delay_ms(&runtime.config)),
            action: RenderCleanupAppliedAction::SoftCleared {
                retained_resources: 0,
            },
        }),
    );
    assert_cooling_cleanup_state(
        after_soft.next.render_cleanup(),
        Millis::new(79 + render_cleanup_delay_ms(&runtime.config)),
        Millis::new(79 + render_cleanup_delay_ms(&runtime.config)),
        Millis::new(79 + render_hard_cleanup_delay_ms(&runtime.config)),
    );
    pretty_assert_eq!(
        after_soft.next.timers().active_token(TimerId::Cleanup),
        None
    );
    pretty_assert_eq!(
        after_soft.effects,
        vec![Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
            execution: RenderCleanupExecution::CompactToBudget {
                target_budget: 2,
                max_teardown_attempts_per_tick:
                    crate::core::runtime_reducer::render_cleanup_max_teardown_attempts_per_tick(
                        &runtime.config,
                    ),
            },
        })]
    );

    let after_compaction = reduce(
        &after_soft.next,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at: Millis::new(79 + render_cleanup_delay_ms(&runtime.config)),
            action: RenderCleanupAppliedAction::CompactedToBudget {
                converged_to_idle: true,
                progress: RenderCleanupCompactionProgress::NoProgress,
            },
        }),
    );

    pretty_assert_eq!(
        after_compaction
            .next
            .timers()
            .active_token(TimerId::Cleanup),
        None
    );
    assert_cold_cleanup_state(after_compaction.next.render_cleanup());
    pretty_assert_eq!(
        after_compaction.effects,
        vec![Effect::RecordEventLoopMetric(
            EventLoopMetricEffect::CleanupConvergedToCold {
                started_at: Millis::new(79 + render_cleanup_delay_ms(&runtime.config)),
                converged_at: Millis::new(79 + render_cleanup_delay_ms(&runtime.config)),
            },
        )]
    );
}

#[test]
fn hard_deadline_switches_to_bounded_zero_budget_compaction() {
    let mut runtime = ready_state_with_observation(cursor(4, 9)).runtime().clone();
    runtime.config.max_kept_windows = 21;
    let state = ready_state_with_observation(cursor(4, 9)).with_runtime(runtime.clone());
    let basis = PatchBasis::new(None, None);
    let patch = ScenePatch::derive(basis);
    let (state, proposal_id) = state.allocate_proposal_id();
    let proposal = InFlightProposal::clear(
        proposal_id,
        patch,
        RealizationClear::new(21),
        RenderCleanupAction::Schedule,
        RenderSideEffects::default(),
        crate::core::state::AnimationSchedule::Idle,
    )
    .expect("clear proposal should be constructible");
    let staged = state
        .enter_planning(proposal_id)
        .expect("staging clear proposal requires a ready observation")
        .enter_applying(proposal)
        .expect("staging clear proposal requires the matching planning proposal");

    let completed = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(79),
            visual_change: true,
        }),
    );
    let soft_token = completed
        .next
        .timers()
        .active_token(TimerId::Cleanup)
        .expect("soft cleanup timer should be armed");
    let soft_tick = reduce(
        &completed.next,
        cleanup_tick_event(soft_token, 79 + render_cleanup_delay_ms(&runtime.config)),
    );
    let after_soft = reduce(
        &soft_tick.next,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at: Millis::new(79 + render_cleanup_delay_ms(&runtime.config)),
            action: RenderCleanupAppliedAction::SoftCleared {
                retained_resources: 0,
            },
        }),
    );
    pretty_assert_eq!(
        after_soft.next.timers().active_token(TimerId::Cleanup),
        None
    );
    let after_compaction = reduce(
        &after_soft.next,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at: Millis::new(79 + render_cleanup_delay_ms(&runtime.config)),
            action: RenderCleanupAppliedAction::CompactedToBudget {
                converged_to_idle: false,
                progress: RenderCleanupCompactionProgress::NoProgress,
            },
        }),
    );
    assert_cooling_cleanup_state(
        after_compaction.next.render_cleanup(),
        Millis::new(79 + render_cleanup_delay_ms(&runtime.config)),
        Millis::new(79 + render_hard_cleanup_delay_ms(&runtime.config)),
        Millis::new(79 + render_hard_cleanup_delay_ms(&runtime.config)),
    );

    let hard_token = after_compaction
        .next
        .timers()
        .active_token(TimerId::Cleanup)
        .expect("hard purge fallback timer should stay armed while cooling remains pending");
    pretty_assert_eq!(
        after_compaction.effects,
        vec![Effect::ScheduleTimer(ScheduleTimerEffect {
            token: hard_token,
            delay: DelayBudgetMs::try_new(
                render_hard_cleanup_delay_ms(&runtime.config)
                    .saturating_sub(render_cleanup_delay_ms(&runtime.config)),
            )
            .expect("hard cleanup fallback delay"),
            requested_at: Millis::new(79 + render_cleanup_delay_ms(&runtime.config)),
        })]
    );

    let hard_tick = reduce(
        &after_compaction.next,
        cleanup_tick_event(
            hard_token,
            79 + render_hard_cleanup_delay_ms(&runtime.config),
        ),
    );

    pretty_assert_eq!(
        hard_tick.effects,
        vec![Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
            execution: RenderCleanupExecution::CompactToZero {
                max_teardown_attempts_per_tick:
                    crate::core::runtime_reducer::render_cleanup_max_teardown_attempts_per_tick(
                        &runtime.config,
                    ),
            },
        })]
    );

    let after_hard = reduce(
        &hard_tick.next,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at: Millis::new(79 + render_hard_cleanup_delay_ms(&runtime.config)),
            action: RenderCleanupAppliedAction::CompactedToBudget {
                converged_to_idle: true,
                progress: RenderCleanupCompactionProgress::NoProgress,
            },
        }),
    );

    pretty_assert_eq!(
        after_hard.next.timers().active_token(TimerId::Cleanup),
        None
    );
    assert_cold_cleanup_state(after_hard.next.render_cleanup());
    pretty_assert_eq!(
        after_hard.effects,
        vec![Effect::RecordEventLoopMetric(
            EventLoopMetricEffect::CleanupConvergedToCold {
                started_at: Millis::new(79 + render_cleanup_delay_ms(&runtime.config)),
                converged_at: Millis::new(79 + render_hard_cleanup_delay_ms(&runtime.config)),
            },
        )]
    );
}

#[test]
fn non_converged_compaction_with_progress_rearms_after_cleanup_cadence() {
    let mut runtime = ready_state_with_observation(cursor(4, 9)).runtime().clone();
    runtime.config.max_kept_windows = 21;
    let started_at = Millis::new(79 + render_cleanup_delay_ms(&runtime.config));
    let observed_at = started_at;
    let cooling_cleanup = RenderCleanupState::scheduled(
        Millis::new(79),
        render_cleanup_delay_ms(&runtime.config),
        render_hard_cleanup_delay_ms(&runtime.config),
    )
    .enter_cooling(started_at);
    let state = ready_state_with_observation(cursor(4, 9))
        .with_runtime(runtime.clone())
        .with_render_cleanup(cooling_cleanup);

    let after_compaction = reduce(
        &state,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at,
            action: RenderCleanupAppliedAction::CompactedToBudget {
                converged_to_idle: false,
                progress: RenderCleanupCompactionProgress::MadeProgress,
            },
        }),
    );

    let next_due_at = Millis::new(
        observed_at
            .value()
            .saturating_add(render_cleanup_delay_ms(&runtime.config)),
    );
    let hard_due_at = Millis::new(
        observed_at
            .value()
            .saturating_add(render_hard_cleanup_delay_ms(&runtime.config)),
    );
    assert_cooling_cleanup_state(
        after_compaction.next.render_cleanup(),
        started_at,
        next_due_at,
        hard_due_at,
    );
    let cleanup_token = after_compaction
        .next
        .timers()
        .active_token(TimerId::Cleanup)
        .expect("progressing compaction should schedule another cleanup cadence");
    pretty_assert_eq!(
        after_compaction.effects,
        vec![Effect::ScheduleTimer(ScheduleTimerEffect {
            token: cleanup_token,
            delay: DelayBudgetMs::try_new(render_cleanup_delay_ms(&runtime.config))
                .expect("progress compaction cadence delay"),
            requested_at: observed_at,
        })]
    );
}

#[test]
fn stalled_compaction_after_hard_deadline_emits_zero_budget_compaction_without_timer() {
    let runtime = ready_state_with_observation(cursor(4, 9)).runtime().clone();
    let max_teardown_attempts_per_tick =
        crate::core::runtime_reducer::render_cleanup_max_teardown_attempts_per_tick(
            &runtime.config,
        );
    let cooling_cleanup =
        RenderCleanupState::scheduled(Millis::new(79), 30, 90).enter_cooling(Millis::new(109));
    let state = ready_state_with_observation(cursor(4, 9))
        .with_runtime(runtime)
        .with_render_cleanup(cooling_cleanup);

    let after_compaction = reduce(
        &state,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at: Millis::new(170),
            action: RenderCleanupAppliedAction::CompactedToBudget {
                converged_to_idle: false,
                progress: RenderCleanupCompactionProgress::NoProgress,
            },
        }),
    );

    pretty_assert_eq!(
        after_compaction
            .next
            .timers()
            .active_token(TimerId::Cleanup),
        None
    );
    pretty_assert_eq!(
        after_compaction.effects,
        vec![Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
            execution: RenderCleanupExecution::CompactToZero {
                max_teardown_attempts_per_tick,
            },
        })]
    );
}

#[test]
fn cleanup_cold_convergence_releases_runtime_purgeable_storage() {
    let retained_runtime = runtime_with_retained_cleanup_cold_storage(
        ready_state_with_observation(cursor(4, 9)).runtime().clone(),
    );
    let cooling_cleanup =
        RenderCleanupState::scheduled(Millis::new(79), 30, 90).enter_cooling(Millis::new(109));
    let state = ready_state_with_observation(cursor(4, 9))
        .with_runtime(retained_runtime)
        .with_render_cleanup(cooling_cleanup);

    pretty_assert_eq!(
        runtime_cleanup_cold_storage_residency(state.runtime()),
        RuntimeCleanupColdStorageResidency::RETAINED
    );

    let completed = reduce(
        &state,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at: Millis::new(110),
            action: RenderCleanupAppliedAction::CompactedToBudget {
                converged_to_idle: true,
                progress: RenderCleanupCompactionProgress::NoProgress,
            },
        }),
    );

    assert_cold_cleanup_state(completed.next.render_cleanup());
    pretty_assert_eq!(
        runtime_cleanup_cold_storage_residency(completed.next.runtime()),
        RuntimeCleanupColdStorageResidency::RELEASED
    );
}

#[test]
fn stalled_cleanup_keeps_cleanup_retryable_and_storage_retained() {
    let retained_runtime = runtime_with_retained_cleanup_cold_storage(
        ready_state_with_observation(cursor(4, 9)).runtime().clone(),
    );
    let retry_delay_ms = render_cleanup_delay_ms(&retained_runtime.config);
    let hot_cleanup = RenderCleanupState::scheduled(Millis::new(79), 30, 90);
    let state = ready_state_with_observation(cursor(4, 9))
        .with_runtime(retained_runtime)
        .with_render_cleanup(hot_cleanup);
    let observed_at = Millis::new(169);

    let completed = reduce(
        &state,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at,
            action: RenderCleanupAppliedAction::CleanupStalled,
        }),
    );

    let retry_due_at = Millis::new(observed_at.value().saturating_add(retry_delay_ms));
    assert_cooling_cleanup_state(
        completed.next.render_cleanup(),
        observed_at,
        retry_due_at,
        retry_due_at,
    );
    pretty_assert_eq!(
        runtime_cleanup_cold_storage_residency(completed.next.runtime()),
        RuntimeCleanupColdStorageResidency::RETAINED
    );
    let cleanup_token = completed
        .next
        .timers()
        .active_token(TimerId::Cleanup)
        .expect("retained hard purge resources should schedule a retry");
    pretty_assert_eq!(
        completed.effects,
        vec![Effect::ScheduleTimer(ScheduleTimerEffect {
            token: cleanup_token,
            delay: DelayBudgetMs::try_new(retry_delay_ms).expect("hard purge retry delay"),
            requested_at: observed_at,
        })]
    );
}

#[test]
fn stalled_cleanup_retries_back_off_exponentially_and_cap_at_thirty_seconds() {
    let runtime = ready_state_with_observation(cursor(/*row*/ 4, /*col*/ 9))
        .runtime()
        .clone();
    let max_teardown_attempts_per_tick =
        crate::core::runtime_reducer::render_cleanup_max_teardown_attempts_per_tick(
            &runtime.config,
        );
    let cleanup = RenderCleanupState::scheduled(
        Millis::new(/*value*/ 79),
        /*soft_delay_ms*/ 30,
        /*hard_delay_ms*/ 90,
    );
    let mut state = ready_state_with_observation(cursor(/*row*/ 4, /*col*/ 9))
        .with_runtime(runtime)
        .with_render_cleanup(cleanup);
    let mut observed_at = Millis::new(/*value*/ 10_000);
    let mut actual = Vec::new();

    for expected_attempt in 1..=10 {
        let completed = reduce(
            &state,
            Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
                observed_at,
                action: RenderCleanupAppliedAction::CleanupStalled,
            }),
        );
        let due_at = completed
            .next
            .render_cleanup()
            .next_deadline()
            .expect("retained resources should keep cleanup retryable");
        let [Effect::ScheduleTimer(schedule)] = completed.effects.as_slice() else {
            panic!("retained resources should schedule exactly one cleanup retry");
        };
        actual.push((
            completed
                .next
                .render_cleanup()
                .retained_resource_retry_attempt(),
            due_at.value().saturating_sub(observed_at.value()),
            schedule.delay.value(),
        ));

        if expected_attempt < 10 {
            let fired = reduce(
                &completed.next,
                cleanup_tick_event(schedule.token, due_at.value()),
            );
            let [
                Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
                    execution:
                        RenderCleanupExecution::CompactToZero {
                            max_teardown_attempts_per_tick: actual_max_teardown_attempts_per_tick,
                        },
                }),
            ] = fired.effects.as_slice()
            else {
                panic!("a retained-resource retry deadline should request one hard purge");
            };
            pretty_assert_eq!(
                *actual_max_teardown_attempts_per_tick,
                max_teardown_attempts_per_tick
            );
            state = fired.next;
            observed_at = due_at;
        }
    }

    pretty_assert_eq!(
        actual,
        vec![
            (1, 200, 200),
            (2, 400, 400),
            (3, 800, 800),
            (4, 1_600, 1_600),
            (5, 3_200, 3_200),
            (6, 6_400, 6_400),
            (7, 12_800, 12_800),
            (8, 25_600, 25_600),
            (9, 30_000, 30_000),
            (10, 30_000, 30_000),
        ]
    );
}

#[test]
fn retained_soft_clear_resources_skip_compaction_and_schedule_hard_purge_retry() {
    let retained_runtime = runtime_with_retained_cleanup_cold_storage(
        ready_state_with_observation(cursor(4, 9)).runtime().clone(),
    );
    let retry_delay_ms = render_cleanup_delay_ms(&retained_runtime.config);
    let hot_cleanup = RenderCleanupState::scheduled(Millis::new(79), 30, 90);
    let state = ready_state_with_observation(cursor(4, 9))
        .with_runtime(retained_runtime)
        .with_render_cleanup(hot_cleanup);
    let observed_at = Millis::new(109);

    let completed = reduce(
        &state,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at,
            action: RenderCleanupAppliedAction::SoftCleared {
                retained_resources: 1,
            },
        }),
    );

    let retry_due_at = Millis::new(observed_at.value().saturating_add(retry_delay_ms));
    assert_cooling_cleanup_state(
        completed.next.render_cleanup(),
        observed_at,
        retry_due_at,
        retry_due_at,
    );
    pretty_assert_eq!(
        runtime_cleanup_cold_storage_residency(completed.next.runtime()),
        RuntimeCleanupColdStorageResidency::RETAINED
    );
    let cleanup_token = completed
        .next
        .timers()
        .active_token(TimerId::Cleanup)
        .expect("retained soft clear resources should schedule a hard-purge retry");
    pretty_assert_eq!(
        completed.effects,
        vec![Effect::ScheduleTimer(ScheduleTimerEffect {
            token: cleanup_token,
            delay: DelayBudgetMs::try_new(retry_delay_ms).expect("soft clear retry delay"),
            requested_at: observed_at,
        })]
    );
}

#[test]
fn retained_cleanup_resources_observed_schedules_retry_without_fake_apply_completion() {
    let retained_runtime = runtime_with_retained_cleanup_cold_storage(
        ready_state_with_observation(cursor(4, 9)).runtime().clone(),
    );
    let retry_delay_ms = render_cleanup_delay_ms(&retained_runtime.config);
    let state = ready_state_with_observation(cursor(4, 9))
        .with_runtime(retained_runtime)
        .with_render_cleanup(RenderCleanupState::cold());
    let observed_at = Millis::new(219);

    let completed = reduce(
        &state,
        Event::RenderCleanupRetainedResourcesObserved(
            RenderCleanupRetainedResourcesObservedEvent {
                observed_at,
                retained_resources: 3,
            },
        ),
    );

    let retry_due_at = Millis::new(observed_at.value().saturating_add(retry_delay_ms));
    assert_cooling_cleanup_state(
        completed.next.render_cleanup(),
        observed_at,
        retry_due_at,
        retry_due_at,
    );
    pretty_assert_eq!(
        runtime_cleanup_cold_storage_residency(completed.next.runtime()),
        RuntimeCleanupColdStorageResidency::RETAINED
    );
    let cleanup_token = completed
        .next
        .timers()
        .active_token(TimerId::Cleanup)
        .expect("retained resource observation should schedule a cleanup retry");
    pretty_assert_eq!(
        completed.effects,
        vec![Effect::ScheduleTimer(ScheduleTimerEffect {
            token: cleanup_token,
            delay: DelayBudgetMs::try_new(retry_delay_ms).expect("retained resource retry delay"),
            requested_at: observed_at,
        })]
    );
}

#[test]
fn retained_cleanup_resources_observed_while_hot_after_deadline_keeps_active_timer_owner() {
    let mut retained_runtime = runtime_with_retained_cleanup_cold_storage(
        ready_state_with_observation(cursor(4, 9)).runtime().clone(),
    );
    retained_runtime.config.max_kept_windows = 21;
    let hot_cleanup = RenderCleanupState::scheduled(Millis::new(100), 30, 90);
    let state = ready_state_with_observation(cursor(4, 9))
        .with_runtime(retained_runtime)
        .with_render_cleanup(hot_cleanup);
    let (timers, cleanup_token) = state.timers().arm(TimerId::Cleanup);
    let state = state.with_timers(timers);
    let observed_at = Millis::new(135);

    let completed = reduce(
        &state,
        Event::RenderCleanupRetainedResourcesObserved(
            RenderCleanupRetainedResourcesObservedEvent {
                observed_at,
                retained_resources: 3,
            },
        ),
    );

    assert_hot_cleanup_state(
        completed.next.render_cleanup(),
        Millis::new(130),
        Millis::new(190),
    );
    pretty_assert_eq!(
        runtime_cleanup_cold_storage_residency(completed.next.runtime()),
        RuntimeCleanupColdStorageResidency::RETAINED
    );
    pretty_assert_eq!(
        completed.next.timers().active_token(TimerId::Cleanup),
        Some(cleanup_token)
    );
    pretty_assert_eq!(completed.effects, Vec::<Effect>::new());

    let timer_fired = reduce(
        &completed.next,
        cleanup_tick_event(cleanup_token, observed_at.value()),
    );

    pretty_assert_eq!(
        timer_fired.effects,
        vec![Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
            execution: RenderCleanupExecution::SoftClear {
                max_kept_windows: 21,
            },
        })]
    );
}

#[test]
fn cleanup_effects_follow_current_runtime_config_without_scheduler_policy_copies() {
    let mut runtime = ready_state_with_observation(cursor(4, 9)).runtime().clone();
    runtime.config.max_kept_windows = 21;
    let state = ready_state_with_observation(cursor(4, 9)).with_runtime(runtime);
    let basis = PatchBasis::new(None, None);
    let patch = ScenePatch::derive(basis);
    let (state, proposal_id) = state.allocate_proposal_id();
    let proposal = InFlightProposal::clear(
        proposal_id,
        patch,
        RealizationClear::new(21),
        RenderCleanupAction::Schedule,
        RenderSideEffects::default(),
        crate::core::state::AnimationSchedule::Idle,
    )
    .expect("clear proposal should be constructible");
    let staged = state
        .enter_planning(proposal_id)
        .expect("staging clear proposal requires a ready observation")
        .enter_applying(proposal)
        .expect("staging clear proposal requires the matching planning proposal");

    let completed = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(79),
            visual_change: true,
        }),
    );
    let mut hot_runtime = completed.next.runtime().clone();
    hot_runtime.config.max_kept_windows = 7;
    let hot_state = completed.next.with_runtime(hot_runtime.clone());
    let soft_token = hot_state
        .timers()
        .active_token(TimerId::Cleanup)
        .expect("soft cleanup timer should be armed");

    let soft_tick = reduce(
        &hot_state,
        cleanup_tick_event(
            soft_token,
            79 + render_cleanup_delay_ms(&hot_runtime.config),
        ),
    );

    pretty_assert_eq!(
        soft_tick.effects,
        vec![Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
            execution: RenderCleanupExecution::SoftClear {
                max_kept_windows: 7,
            },
        })]
    );

    let mut cooling_runtime = soft_tick.next.runtime().clone();
    cooling_runtime.config.max_kept_windows = 1;
    let cooling_state = soft_tick.next.with_runtime(cooling_runtime);
    let after_soft = reduce(
        &cooling_state,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at: Millis::new(79 + render_cleanup_delay_ms(&hot_runtime.config)),
            action: RenderCleanupAppliedAction::SoftCleared {
                retained_resources: 0,
            },
        }),
    );

    assert_cooling_cleanup_state(
        after_soft.next.render_cleanup(),
        Millis::new(79 + render_cleanup_delay_ms(&hot_runtime.config)),
        Millis::new(79 + render_cleanup_delay_ms(&hot_runtime.config)),
        Millis::new(79 + render_hard_cleanup_delay_ms(&hot_runtime.config)),
    );
    pretty_assert_eq!(
        after_soft.effects,
        vec![Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
            execution: RenderCleanupExecution::CompactToBudget {
                target_budget: 1,
                max_teardown_attempts_per_tick: 1,
            },
        })]
    );
}

#[test]
fn fresh_ingress_promotes_cooling_cleanup_state_back_to_hot() {
    let mut runtime = ready_state_with_observation(cursor(4, 9)).runtime().clone();
    runtime.config.max_kept_windows = 21;
    let state = ready_state_with_observation(cursor(4, 9)).with_runtime(runtime.clone());
    let basis = PatchBasis::new(None, None);
    let patch = ScenePatch::derive(basis);
    let (state, proposal_id) = state.allocate_proposal_id();
    let proposal = InFlightProposal::clear(
        proposal_id,
        patch,
        RealizationClear::new(21),
        RenderCleanupAction::Schedule,
        RenderSideEffects::default(),
        crate::core::state::AnimationSchedule::Idle,
    )
    .expect("clear proposal should be constructible");
    let staged = state
        .enter_planning(proposal_id)
        .expect("staging clear proposal requires a ready observation")
        .enter_applying(proposal)
        .expect("staging clear proposal requires the matching planning proposal");

    let completed = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(79),
            visual_change: true,
        }),
    );
    let soft_token = completed
        .next
        .timers()
        .active_token(TimerId::Cleanup)
        .expect("soft cleanup timer should be armed");
    let soft_tick = reduce(
        &completed.next,
        cleanup_tick_event(soft_token, 79 + render_cleanup_delay_ms(&runtime.config)),
    );
    let cooling = reduce(
        &soft_tick.next,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at: Millis::new(79 + render_cleanup_delay_ms(&runtime.config)),
            action: RenderCleanupAppliedAction::SoftCleared {
                retained_resources: 0,
            },
        }),
    )
    .next;

    let reheated = reduce(
        &cooling,
        external_demand_event(ExternalDemandKind::BufferEntered, 150),
    );

    pretty_assert_eq!(
        reheated.next.render_cleanup().thermal(),
        RenderThermalState::Hot
    );
    assert!(
        reheated
            .next
            .timers()
            .active_token(TimerId::Cleanup)
            .is_some(),
        "fresh ingress should keep one cleanup timer alive while hot deadlines move forward"
    );
}
