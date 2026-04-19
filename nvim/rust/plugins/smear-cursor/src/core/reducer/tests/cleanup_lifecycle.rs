use super::*;

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
        .enter_applying(proposal)
        .expect("staging clear proposal requires retained observation");

    let completed = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(79),
            visual_change: true,
        }),
    );
    pretty_assert_eq!(
        completed.next.render_cleanup().thermal(),
        RenderThermalState::Hot
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
            action: RenderCleanupAppliedAction::SoftCleared,
        }),
    );
    pretty_assert_eq!(
        after_soft.next.render_cleanup().thermal(),
        RenderThermalState::Cooling
    );
    pretty_assert_eq!(
        after_soft.next.render_cleanup().entered_cooling_at(),
        Some(Millis::new(79 + render_cleanup_delay_ms(&runtime.config)))
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
                max_prune_per_tick: 21,
            },
        })]
    );

    let after_compaction = reduce(
        &after_soft.next,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at: Millis::new(79 + render_cleanup_delay_ms(&runtime.config)),
            action: RenderCleanupAppliedAction::CompactedToBudget {
                converged_to_idle: true,
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
        after_compaction.next.render_cleanup().thermal(),
        RenderThermalState::Cold
    );
    pretty_assert_eq!(
        after_compaction.next.render_cleanup().idle_target_budget(),
        2
    );
    pretty_assert_eq!(
        after_compaction.next.render_cleanup().max_kept_windows(),
        21
    );
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
fn hard_purge_stays_as_fallback_when_cooling_compaction_does_not_converge() {
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
        .enter_applying(proposal)
        .expect("staging clear proposal requires retained observation");

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
            action: RenderCleanupAppliedAction::SoftCleared,
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
            },
        }),
    );
    pretty_assert_eq!(
        after_compaction.next.render_cleanup().thermal(),
        RenderThermalState::Cooling
    );

    let hard_token = after_compaction
        .next
        .timers()
        .active_token(TimerId::Cleanup)
        .expect("hard purge fallback timer should stay armed while cooling remains pending");

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
            execution: RenderCleanupExecution::HardPurge,
        })]
    );

    let after_hard = reduce(
        &hard_tick.next,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at: Millis::new(79 + render_hard_cleanup_delay_ms(&runtime.config)),
            action: RenderCleanupAppliedAction::HardPurged,
        }),
    );

    pretty_assert_eq!(
        after_hard.next.timers().active_token(TimerId::Cleanup),
        None
    );
    pretty_assert_eq!(
        after_hard.next.render_cleanup().thermal(),
        RenderThermalState::Cold
    );
    pretty_assert_eq!(after_hard.next.render_cleanup().idle_target_budget(), 2);
    pretty_assert_eq!(after_hard.next.render_cleanup().max_kept_windows(), 21);
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
        .enter_applying(proposal)
        .expect("staging clear proposal requires retained observation");

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
            action: RenderCleanupAppliedAction::SoftCleared,
        }),
    )
    .next;

    let reheated = reduce(
        &cooling,
        external_demand_event(ExternalDemandKind::BufferEntered, 150, None),
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

#[test]
fn diverged_realization_cannot_derive_noop_for_identical_target() {
    let (staged, proposal_id) =
        planned_state_after_animation_tick(ready_state_with_observation(cursor(9, 9)), 86);
    let target = staged
        .pending_proposal()
        .and_then(|proposal| proposal.patch().basis().target().cloned())
        .expect("target projection for divergence noop regression");
    let ready = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(87),
            visual_change: true,
        }),
    )
    .next;
    let diverged = ready.with_realization(RealizationLedger::diverged_from(
        Some(target.clone()),
        RealizationDivergence::ShellStateUnknown,
    ));

    let patch = ScenePatch::derive(PatchBasis::new(
        diverged
            .realization()
            .trusted_acknowledged_for_patch()
            .cloned(),
        Some(target),
    ));

    pretty_assert_eq!(patch.kind(), crate::core::state::ScenePatchKind::Replace);
}
