use super::super::*;

pub(in crate::core::reducer::tests) fn noop_realization_plan() -> RealizationPlan {
    RealizationPlan::Noop
}

pub(in crate::core::reducer::tests) fn with_cleanup_invalidation(
    next_state: &CoreState,
    observed_at: u64,
    mut effects: Vec<Effect>,
) -> Vec<Effect> {
    let cleanup_token = next_state
        .timers()
        .active_token(TimerId::Cleanup)
        .expect("cleanup timer should be armed");
    effects.push(Effect::ScheduleTimer(ScheduleTimerEffect {
        token: cleanup_token,
        delay: DelayBudgetMs::try_new(render_cleanup_delay_ms(&next_state.runtime().config))
            .expect("cleanup delay budget"),
        requested_at: Millis::new(observed_at),
    }));
    effects
}

pub(in crate::core::reducer::tests) fn timer_armed_state(
    state: CoreState,
) -> (CoreState, TimerToken) {
    let (timers, token) = state.timers().arm(TimerId::Animation);
    (state.with_timers(timers), token)
}

pub(in crate::core::reducer::tests) fn animation_tick_event(
    token: TimerToken,
    observed_at: u64,
) -> Event {
    Event::TimerFiredWithToken(TimerFiredWithTokenEvent {
        token,
        observed_at: Millis::new(observed_at),
    })
}

pub(in crate::core::reducer::tests) fn cleanup_tick_event(
    token: TimerToken,
    observed_at: u64,
) -> Event {
    Event::TimerFiredWithToken(TimerFiredWithTokenEvent {
        token,
        observed_at: Millis::new(observed_at),
    })
}

pub(in crate::core::reducer::tests) fn planned_state_after_animation_tick(
    state: CoreState,
    observed_at: u64,
) -> (CoreState, ProposalId) {
    let (armed_state, token) = timer_armed_state(state);
    let transition = reduce(&armed_state, animation_tick_event(token, observed_at));
    let Effect::RequestRenderPlan(payload) = transition
        .effects
        .into_iter()
        .next()
        .expect("render plan request after animation tick")
    else {
        panic!("expected render plan request after animation tick");
    };
    let proposal_id = payload.proposal_id;
    let computed = reduce(
        &transition.next,
        Event::RenderPlanComputed(RenderPlanComputedEvent {
            proposal_id,
            planned_render: Box::new(
                crate::core::reducer::build_planned_render(
                    &payload.planning_state,
                    payload.proposal_id,
                    &payload.render_decision,
                    payload.animation_schedule,
                )
                .expect("planned render should satisfy proposal shape invariants"),
            ),
            observed_at: payload.requested_at,
        }),
    );
    (computed.next, proposal_id)
}

pub(in crate::core::reducer::tests) fn applying_state_with_realization_plan(
    state: CoreState,
    realization: RealizationPlan,
    should_schedule_next_animation: bool,
    next_animation_at_ms: Option<Millis>,
) -> (CoreState, ProposalId) {
    let acknowledged = state
        .realization()
        .trusted_acknowledged_for_patch()
        .cloned();
    let target = match &realization {
        RealizationPlan::Draw(_) => acknowledged.clone().or_else(|| {
            state
                .scene()
                .projection_entry()
                .map(|entry| entry.snapshot().clone())
        }),
        RealizationPlan::Noop => acknowledged.clone(),
        RealizationPlan::Clear(_) | RealizationPlan::Failure(_) => None,
    };
    let basis = PatchBasis::new(acknowledged, target);
    let patch = ScenePatch::derive(basis);
    let (state, proposal_id) = state.allocate_proposal_id();
    let proposal = match realization {
        RealizationPlan::Draw(draw) => InFlightProposal::draw(
            proposal_id,
            patch,
            draw,
            RenderCleanupAction::NoAction,
            RenderSideEffects::default(),
            crate::core::state::AnimationSchedule::from_parts(
                should_schedule_next_animation,
                next_animation_at_ms,
            ),
        )
        .expect("test draw proposal should be constructible"),
        RealizationPlan::Clear(clear) => InFlightProposal::clear(
            proposal_id,
            patch,
            clear,
            RenderCleanupAction::NoAction,
            RenderSideEffects::default(),
            crate::core::state::AnimationSchedule::from_parts(
                should_schedule_next_animation,
                next_animation_at_ms,
            ),
        )
        .expect("test clear proposal should be constructible"),
        RealizationPlan::Noop => InFlightProposal::noop(
            proposal_id,
            patch,
            RenderCleanupAction::NoAction,
            RenderSideEffects::default(),
            crate::core::state::AnimationSchedule::from_parts(
                should_schedule_next_animation,
                next_animation_at_ms,
            ),
        )
        .expect("test noop proposal should be constructible"),
        RealizationPlan::Failure(failure) => InFlightProposal::failure(
            proposal_id,
            patch,
            failure,
            RenderCleanupAction::NoAction,
            RenderSideEffects::default(),
            crate::core::state::AnimationSchedule::from_parts(
                should_schedule_next_animation,
                next_animation_at_ms,
            ),
        ),
    };
    (
        state
            .into_applying(proposal)
            .expect("test staging requires a retained observation"),
        proposal_id,
    )
}
