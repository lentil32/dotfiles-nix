use super::*;

#[test]
fn apply_completed_advances_acknowledged_projection() {
    let (staged, proposal_id) =
        planned_state_after_animation_tick(ready_state_with_observation(cursor(9, 9)), 62);
    let acknowledged = staged
        .pending_proposal()
        .and_then(|proposal| proposal.patch().basis().target().cloned())
        .expect("target projection for apply completion");

    let completed = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(63),
            visual_change: true,
        }),
    );

    pretty_assert_eq!(completed.next.lifecycle(), Lifecycle::Ready);
    pretty_assert_eq!(
        completed.next.realization(),
        &RealizationLedger::Consistent { acknowledged }
    );
}

#[test]
fn render_cleanup_applied_clears_trusted_realization_basis() {
    let (staged, proposal_id) =
        planned_state_after_animation_tick(ready_state_with_observation(cursor(9, 9)), 64);
    let acknowledged = staged
        .pending_proposal()
        .and_then(|proposal| proposal.patch().basis().target().cloned())
        .expect("target projection for cleanup");
    let ready = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(65),
            visual_change: true,
        }),
    )
    .next;

    let cleaned = reduce(
        &ready,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at: Millis::new(66),
            action: RenderCleanupAppliedAction::SoftCleared,
        }),
    );

    pretty_assert_eq!(cleaned.next.lifecycle(), Lifecycle::Ready);
    pretty_assert_eq!(
        cleaned.next.realization(),
        &RealizationLedger::Diverged {
            last_consistent: Some(acknowledged.clone()),
            divergence: RealizationDivergence::ShellStateUnknown,
        }
    );
    pretty_assert_eq!(
        cleaned.next.realization().trusted_acknowledged_for_patch(),
        None
    );
    pretty_assert_eq!(
        cleaned.next.realization().last_consistent(),
        Some(&acknowledged)
    );
}

#[test]
fn untrusted_target_basis_derives_replace_patch() {
    let (staged, proposal_id) =
        planned_state_after_animation_tick(ready_state_with_observation(cursor(9, 9)), 67);
    let target = staged
        .pending_proposal()
        .and_then(|proposal| proposal.patch().basis().target().cloned())
        .expect("target projection for cleanup noop regression");
    let ready = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(68),
            visual_change: true,
        }),
    )
    .next;
    let cleaned = reduce(
        &ready,
        Event::RenderCleanupApplied(RenderCleanupAppliedEvent {
            observed_at: Millis::new(69),
            action: RenderCleanupAppliedAction::HardPurged,
        }),
    );

    let patch = ScenePatch::derive(PatchBasis::new(
        cleaned
            .next
            .realization()
            .trusted_acknowledged_for_patch()
            .cloned(),
        Some(target),
    ));

    pretty_assert_eq!(patch.kind(), crate::core::state::ScenePatchKind::Replace);
}

#[test]
fn apply_completion_emits_explicit_cleanup_and_redraw_effects() {
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
        RenderSideEffects {
            redraw_after_clear_if_cmdline: true,
            ..RenderSideEffects::default()
        },
        crate::core::state::AnimationSchedule::Idle,
    )
    .expect("clear proposal should be constructible");
    let staged = state
        .into_applying(proposal)
        .expect("staging clear proposal requires retained observation");

    let completed = reduce(
        &staged,
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(79),
            visual_change: true,
        }),
    );

    pretty_assert_eq!(completed.next.lifecycle(), Lifecycle::Ready);
    pretty_assert_eq!(
        completed.next.render_cleanup().thermal(),
        RenderThermalState::Hot
    );
    pretty_assert_eq!(
        completed.next.render_cleanup().next_compaction_due_at(),
        Some(Millis::new(79 + render_cleanup_delay_ms(&runtime.config)))
    );
    pretty_assert_eq!(completed.next.render_cleanup().entered_cooling_at(), None);
    let cleanup_token = completed
        .next
        .timers()
        .active_token(TimerId::Cleanup)
        .expect("cleanup timer should be armed");
    pretty_assert_eq!(
        completed.effects,
        vec![
            Effect::ScheduleTimer(ScheduleTimerEffect {
                token: cleanup_token,
                delay: DelayBudgetMs::try_new(render_cleanup_delay_ms(&runtime.config))
                    .expect("cleanup delay budget"),
                requested_at: Millis::new(79),
            }),
            Effect::RedrawCmdline,
        ]
    );
}
