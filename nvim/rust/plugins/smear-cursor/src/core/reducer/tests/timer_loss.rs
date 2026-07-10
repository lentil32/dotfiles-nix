use super::*;

#[test]
fn animation_timer_lost_quiesces_runtime_without_render_planning_or_rearm() {
    let base = compatible_cursor_color_ready_state(|runtime| {
        runtime.start_animation();
        runtime.set_last_tick_ms(/*value*/ Some(100.0));
    });
    let (state, token) = timer_armed_state(base);
    let max_kept_windows = state.runtime().config.max_kept_windows;
    let mut expected_runtime = state.runtime().clone();
    expected_runtime.recover_from_clock_discontinuity();
    let expected_state = state
        .clone()
        .with_timers(state.timers().clear_matching(token))
        .with_runtime(expected_runtime);

    let transition = reduce(
        &state,
        Event::TimerLostWithToken(TimerLostWithTokenEvent {
            token,
            observed_at: Millis::new(/*value*/ 10_000),
        }),
    );

    pretty_assert_eq!(
        transition,
        Transition::new(
            expected_state,
            vec![Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
                execution: RenderCleanupExecution::SoftClear { max_kept_windows },
            })],
        )
    );
}

#[test]
fn animation_timer_lost_during_apply_only_disarms_the_interleaved_timer() {
    let (applying, _proposal_id) = applying_state_with_realization_plan(
        ready_state_with_observation(cursor(/*row*/ 4, /*col*/ 9)),
        noop_realization_plan(),
        /*should_schedule_next_animation*/ false,
        /*next_animation_at_ms*/ None,
    );
    let mut runtime = applying.runtime().clone();
    runtime.start_animation();
    runtime.set_last_tick_ms(/*value*/ Some(100.0));
    let (state, token) = timer_armed_state(applying.with_runtime(runtime));
    let expected_state = state
        .clone()
        .with_timers(state.timers().clear_matching(token));

    let transition = reduce(
        &state,
        Event::TimerLostWithToken(TimerLostWithTokenEvent {
            token,
            observed_at: Millis::new(/*value*/ 10_000),
        }),
    );

    pretty_assert_eq!(transition, Transition::stay_owned(expected_state));
}

#[test]
fn cleanup_timer_lost_before_deadline_quiesces_without_clearing_active_rendering() {
    let cleanup = RenderCleanupState::scheduled(
        Millis::new(/*value*/ 100),
        /*soft_delay_ms*/ 30,
        /*hard_delay_ms*/ 90,
    );
    let state =
        ready_state_with_observation(cursor(/*row*/ 4, /*col*/ 9)).with_render_cleanup(cleanup);
    let (timers, token) = state.timers().arm(TimerId::Cleanup);
    let state = state.with_timers(timers);
    let expected_state = state
        .clone()
        .with_timers(state.timers().clear_matching(token))
        .with_render_cleanup(cleanup.quiesce_timer_rearm());

    let transition = reduce(
        &state,
        Event::TimerLostWithToken(TimerLostWithTokenEvent {
            token,
            observed_at: Millis::new(/*value*/ 110),
        }),
    );

    pretty_assert_eq!(transition, Transition::stay_owned(expected_state));
}

#[test]
fn hot_cleanup_timer_lost_requests_one_soft_clear_without_rearm() {
    let cleanup = RenderCleanupState::scheduled(
        Millis::new(/*value*/ 100),
        /*soft_delay_ms*/ 30,
        /*hard_delay_ms*/ 90,
    );
    let state =
        ready_state_with_observation(cursor(/*row*/ 4, /*col*/ 9)).with_render_cleanup(cleanup);
    let max_kept_windows = state.runtime().config.max_kept_windows;
    let (timers, token) = state.timers().arm(TimerId::Cleanup);
    let state = state.with_timers(timers);
    let expected_state = state
        .clone()
        .with_timers(state.timers().clear_matching(token))
        .with_render_cleanup(cleanup.quiesce_timer_rearm());

    let transition = reduce(
        &state,
        Event::TimerLostWithToken(TimerLostWithTokenEvent {
            token,
            observed_at: Millis::new(/*value*/ 10_000),
        }),
    );

    pretty_assert_eq!(
        transition,
        Transition::new(
            expected_state,
            vec![Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
                execution: RenderCleanupExecution::SoftClear { max_kept_windows },
            })],
        )
    );
}

#[test]
fn cooling_cleanup_timer_lost_requests_one_bounded_compaction_without_rearm() {
    let cleanup = RenderCleanupState::scheduled(
        Millis::new(/*value*/ 100),
        /*soft_delay_ms*/ 30,
        /*hard_delay_ms*/ 90,
    )
    .enter_cooling(Millis::new(/*value*/ 130));
    let state =
        ready_state_with_observation(cursor(/*row*/ 4, /*col*/ 9)).with_render_cleanup(cleanup);
    let target_budget =
        crate::core::runtime_reducer::render_cleanup_idle_target_budget(&state.runtime().config);
    let max_teardown_attempts_per_tick =
        crate::core::runtime_reducer::render_cleanup_max_teardown_attempts_per_tick(
            &state.runtime().config,
        );
    let (timers, token) = state.timers().arm(TimerId::Cleanup);
    let state = state.with_timers(timers);
    let expected_state = state
        .clone()
        .with_timers(state.timers().clear_matching(token))
        .with_render_cleanup(cleanup.quiesce_timer_rearm());

    let transition = reduce(
        &state,
        Event::TimerLostWithToken(TimerLostWithTokenEvent {
            token,
            observed_at: Millis::new(/*value*/ 10_000),
        }),
    );

    pretty_assert_eq!(
        transition,
        Transition::new(
            expected_state,
            vec![Effect::ApplyRenderCleanup(ApplyRenderCleanupEffect {
                execution: RenderCleanupExecution::CompactToBudget {
                    target_budget,
                    max_teardown_attempts_per_tick,
                },
            })],
        )
    );
}

#[test]
fn lost_cleanup_timer_stays_quiescent_after_cleanup_application() {
    let cleanup = RenderCleanupState::scheduled(
        Millis::new(/*value*/ 100),
        /*soft_delay_ms*/ 30,
        /*hard_delay_ms*/ 90,
    );
    let state =
        ready_state_with_observation(cursor(/*row*/ 4, /*col*/ 9)).with_render_cleanup(cleanup);
    let hard_cleanup_delay_ms = render_hard_cleanup_delay_ms(&state.runtime().config);
    let (timers, token) = state.timers().arm(TimerId::Cleanup);
    let state = state.with_timers(timers);
    let lost = reduce(
        &state,
        Event::TimerLostWithToken(TimerLostWithTokenEvent {
            token,
            observed_at: Millis::new(/*value*/ 10_000),
        }),
    );
    let completion_observed_at = Millis::new(/*value*/ 10_001);
    let expected_cleanup = cleanup
        .quiesce_timer_rearm()
        .enter_cooling_after_soft_clear(completion_observed_at, hard_cleanup_delay_ms);
    let expected_state = lost
        .next
        .clone()
        .with_realization(lost.next.realization().clone().cleanup_applied())
        .with_render_cleanup(expected_cleanup);

    let completed = reduce(
        &lost.next,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at: completion_observed_at,
            action: RenderCleanupAppliedAction::SoftCleared {
                retained_resources: 0,
            },
        }),
    );

    pretty_assert_eq!(completed, Transition::stay_owned(expected_state));
}

#[test]
fn external_demand_revives_cleanup_timer_after_timer_loss_quiescence() {
    let cleanup = RenderCleanupState::scheduled(
        Millis::new(/*value*/ 100),
        /*soft_delay_ms*/ 30,
        /*hard_delay_ms*/ 90,
    )
    .quiesce_timer_rearm();
    let state =
        ready_state_with_observation(cursor(/*row*/ 4, /*col*/ 9)).with_render_cleanup(cleanup);
    let observed_at = Millis::new(/*value*/ 200);
    let expected_cleanup = RenderCleanupState::scheduled(
        observed_at,
        render_cleanup_delay_ms(&state.runtime().config),
        render_hard_cleanup_delay_ms(&state.runtime().config),
    );

    let transition = reduce(
        &state,
        external_demand_event(
            ExternalDemandKind::ModeChanged,
            /*observed_at*/ observed_at.value(),
        ),
    );
    let cleanup_token = transition
        .next
        .timers()
        .active_token(TimerId::Cleanup)
        .expect("fresh demand should revive cleanup timer ownership");
    let cleanup_schedule_effects = transition
        .effects
        .iter()
        .filter(|effect| {
            matches!(
                effect,
                Effect::ScheduleTimer(payload) if payload.token.id() == TimerId::Cleanup
            )
        })
        .cloned()
        .collect::<Vec<_>>();

    pretty_assert_eq!(
        (
            transition.next.render_cleanup(),
            transition.next.timers().active_token(TimerId::Cleanup),
            cleanup_schedule_effects,
        ),
        (
            expected_cleanup,
            Some(cleanup_token),
            vec![Effect::ScheduleTimer(ScheduleTimerEffect {
                token: cleanup_token,
                delay: DelayBudgetMs::try_new(render_cleanup_delay_ms(&state.runtime().config))
                    .expect("cleanup delay budget"),
                requested_at: observed_at,
            })],
        ),
    );
}

#[test]
fn fresh_cleanup_schedule_revives_timer_ownership_without_moving_deadlines() {
    let cleanup = RenderCleanupState::scheduled(
        Millis::new(/*value*/ 100),
        /*soft_delay_ms*/ 30,
        /*hard_delay_ms*/ 90,
    )
    .quiesce_timer_rearm();
    let state =
        ready_state_with_observation(cursor(/*row*/ 4, /*col*/ 9)).with_render_cleanup(cleanup);
    let (state, proposal_id) = state.allocate_proposal_id();
    let proposal = InFlightProposal::clear(
        proposal_id,
        ScenePatch::derive(PatchBasis::new(None, None)),
        RealizationClear::new(/*max_kept_windows*/ 21),
        RenderCleanupAction::Schedule,
        RenderSideEffects::default(),
        crate::core::state::AnimationSchedule::Idle,
    )
    .expect("cleanup scheduling proposal should be constructible");
    let applying = state
        .enter_planning(proposal_id)
        .expect("cleanup scheduling proposal requires a ready observation")
        .enter_applying(proposal)
        .expect("cleanup scheduling proposal should match the planning proposal");
    let observed_at = Millis::new(/*value*/ 110);

    let transition = reduce(
        &applying,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at,
            visual_change: false,
        }),
    );
    let cleanup_token = transition
        .next
        .timers()
        .active_token(TimerId::Cleanup)
        .expect("fresh cleanup schedule should revive timer ownership");

    pretty_assert_eq!(
        (
            transition.next.lifecycle(),
            transition.next.render_cleanup(),
            transition.effects,
        ),
        (
            Lifecycle::Ready,
            cleanup.revive_timer_rearm(),
            vec![Effect::ScheduleTimer(ScheduleTimerEffect {
                token: cleanup_token,
                delay: DelayBudgetMs::try_new(/*value*/ 20).expect("cleanup delay budget"),
                requested_at: observed_at,
            })],
        ),
    );
}

#[test]
fn retained_resource_observation_preserves_timer_loss_quiescence() {
    let cleanup = RenderCleanupState::scheduled(
        Millis::new(/*value*/ 100),
        /*soft_delay_ms*/ 30,
        /*hard_delay_ms*/ 90,
    )
    .enter_cooling(Millis::new(/*value*/ 130))
    .quiesce_timer_rearm();
    let state =
        ready_state_with_observation(cursor(/*row*/ 4, /*col*/ 9)).with_render_cleanup(cleanup);
    let observed_at = Millis::new(/*value*/ 200);
    let retry_policy =
        crate::core::runtime_reducer::render_cleanup_retry_policy(&state.runtime().config);
    let expected_cleanup =
        cleanup.retry_hard_purge_after_observed_retained_resources(observed_at, retry_policy);
    let expected_state = state
        .clone()
        .with_realization(state.realization().clone().cleanup_applied())
        .with_render_cleanup(expected_cleanup);

    let transition = reduce(
        &state,
        Event::RenderCleanupRetainedResourcesObserved(
            RenderCleanupRetainedResourcesObservedEvent {
                observed_at,
                retained_resources: 1,
            },
        ),
    );

    pretty_assert_eq!(transition, Transition::stay_owned(expected_state));
}

#[test]
fn ingress_timer_lost_processes_pending_demand_without_rescheduling_failed_timer() {
    let ready = ready_state_with_runtime_config(|runtime| {
        runtime.config.delay_event_to_smear = 40.0;
    });
    let delayed = reduce(
        &ready,
        external_demand_event(ExternalDemandKind::ExternalCursor, /*observed_at*/ 20),
    )
    .next;
    let token = delayed
        .timers()
        .active_token(TimerId::Ingress)
        .expect("delayed cursor demand should arm an ingress timer");

    let transition = reduce(
        &delayed,
        Event::TimerLostWithToken(TimerLostWithTokenEvent {
            token,
            observed_at: Millis::new(/*value*/ 21),
        }),
    );
    let [Effect::RequestObservationBase(_)] = transition.effects.as_slice() else {
        panic!("lost ingress timer should process queued demand without scheduling another timer");
    };
    let actual = (
        transition.next.lifecycle(),
        transition.next.ingress_policy().pending_delay_until(),
        transition.next.timers().active_token(TimerId::Ingress),
    );

    pretty_assert_eq!(actual, (Lifecycle::Observing, None, None));
}

#[test]
fn ingress_timer_lost_during_observation_clears_delay_without_reentering_observation() {
    let ready = ready_state_with_runtime_config(|runtime| {
        runtime.config.delay_event_to_smear = 40.0;
    });
    let delayed = reduce(
        &ready,
        external_demand_event(ExternalDemandKind::ExternalCursor, /*observed_at*/ 20),
    )
    .next;
    let token = delayed
        .timers()
        .active_token(TimerId::Ingress)
        .expect("delayed cursor demand should arm an ingress timer");
    let observing = reduce(
        &delayed,
        external_demand_event(ExternalDemandKind::ModeChanged, /*observed_at*/ 21),
    )
    .next;
    pretty_assert_eq!(observing.lifecycle(), Lifecycle::Observing);
    let expected_state = observing
        .clone()
        .with_timers(observing.timers().clear_matching(token))
        .with_ingress_policy(observing.ingress_policy().clear_pending_delay());

    let transition = reduce(
        &observing,
        Event::TimerLostWithToken(TimerLostWithTokenEvent {
            token,
            observed_at: Millis::new(/*value*/ 22),
        }),
    );

    pretty_assert_eq!(transition, Transition::stay_owned(expected_state));
}

#[test]
fn recovery_timer_lost_quiesces_and_retains_the_interrupted_demand() {
    let recovering = recovering_state_with_observation(cursor(/*row*/ 4, /*col*/ 9));
    let queued = reduce(
        &recovering,
        external_demand_event(ExternalDemandKind::ExternalCursor, /*observed_at*/ 20),
    )
    .next;
    let (timers, token) = queued.timers().arm(TimerId::Recovery);
    let state = queued.with_timers(timers);

    let transition = reduce(
        &state,
        Event::TimerLostWithToken(TimerLostWithTokenEvent {
            token,
            observed_at: Millis::new(/*value*/ 21),
        }),
    );

    pretty_assert_eq!(
        (
            transition.next.lifecycle(),
            transition.next.timers().active_token(TimerId::Recovery),
            transition.next.recovery_policy().retry_attempt(),
            transition.next.demand_queue().latest_cursor().is_some(),
            transition.effects,
        ),
        (Lifecycle::Ready, None, 0, true, Vec::new()),
    );
}
