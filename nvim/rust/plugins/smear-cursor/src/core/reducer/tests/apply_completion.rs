use super::*;
use crate::host::BufferHandle;

#[test]
fn render_plan_completion_threads_observation_buffer_handle_into_apply_effect() {
    let (armed_state, token) = timer_armed_state(ready_state_with_observation(cursor(9, 9)));
    let transition = reduce(&armed_state, animation_tick_event(token, 62));
    let Effect::RequestRenderPlan(payload) = transition
        .effects
        .into_iter()
        .next()
        .expect("render plan request after animation tick")
    else {
        panic!("expected render plan request after animation tick");
    };
    let computed = reduce(
        &transition.next,
        Event::RenderPlanComputed(RenderPlanComputedEvent {
            planned_render: Box::new(
                crate::core::reducer::build_planned_render(
                    payload.planning,
                    payload.proposal_id,
                    &payload.render_decision,
                    payload.animation_schedule,
                )
                .expect("planned render should satisfy proposal shape invariants"),
            ),
            observed_at: payload.requested_at,
        }),
    );

    pretty_assert_eq!(
        computed.effects,
        vec![Effect::ApplyProposal(Box::new(
            crate::core::effect::ApplyProposalEffect {
                proposal: computed
                    .next
                    .pending_proposal()
                    .cloned()
                    .expect("render plan completion should stage a proposal"),
                buffer_handle: Some(BufferHandle::from_raw_for_test(/*value*/ 22)),
                requested_at: Millis::new(62),
            }
        ))]
    );
}

#[test]
fn apply_completed_advances_acknowledged_projection() {
    let (staged, proposal_id) =
        planned_state_after_animation_tick(ready_state_with_observation(cursor(9, 9)), 62);
    let acknowledged = staged
        .pending_proposal()
        .and_then(|proposal| proposal.patch().basis().target_handle().cloned())
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
        &RealizationLedger::Consistent {
            acknowledged: acknowledged.clone(),
        }
    );
    assert!(
        completed
            .next
            .realization()
            .trusted_acknowledged_for_patch()
            .expect("acknowledged realization handle after apply completion")
            .ptr_eq(&acknowledged)
    );
}

#[test]
fn render_cleanup_applied_clears_trusted_realization_basis() {
    let (staged, proposal_id) =
        planned_state_after_animation_tick(ready_state_with_observation(cursor(9, 9)), 64);
    let acknowledged = staged
        .pending_proposal()
        .and_then(|proposal| proposal.patch().basis().target_handle().cloned())
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
            action: RenderCleanupAppliedAction::SoftCleared {
                retained_resources: 0,
            },
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
