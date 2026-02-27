mod apply;
mod observation;
mod planning;
mod support;
mod timers;

use super::transition::Transition;
use crate::core::event::{
    ApplyReport, EffectFailedEvent, Event, ExternalDemandQueuedEvent, InitializeEvent,
    ObservationBaseCollectedEvent, TimerFiredWithTokenEvent,
};
use crate::core::state::{
    CoreState, ExternalDemand, ObservationRequest, ProbeRequestSet, RealizationDivergence,
};
use crate::core::types::{
    CursorCol, CursorPosition, CursorRow, Millis, ProposalId, TimerGeneration, TimerId, TimerToken,
};
use apply::{
    reduce_apply_reported, reduce_effect_failed, reduce_render_cleanup_applied,
    reduce_render_plan_computed, reduce_render_plan_failed,
};
use observation::{
    reduce_external_demand_queued, reduce_initialize, reduce_key_fallback_queued,
    reduce_observation_base_collected, reduce_probe_reported,
};
pub(crate) use planning::build_planned_render;
use timers::{reduce_timer_fired_with_token, reduce_timer_lost_with_token};

pub(crate) fn reduce(state: &CoreState, event: Event) -> Transition {
    match event {
        Event::Initialize(payload) => reduce_initialize(state, payload),
        Event::ExternalDemandQueued(payload) => reduce_external_demand_queued(state, payload),
        Event::KeyFallbackQueued(payload) => reduce_key_fallback_queued(state, payload),
        Event::ObservationBaseCollected(payload) => {
            reduce_observation_base_collected(state, payload)
        }
        Event::ProbeReported(payload) => reduce_probe_reported(state, payload),
        Event::RenderPlanComputed(payload) => reduce_render_plan_computed(state, payload),
        Event::RenderPlanFailed(payload) => reduce_render_plan_failed(state, payload),
        Event::ApplyReported(payload) => reduce_apply_reported(state, payload),
        Event::RenderCleanupApplied(payload) => reduce_render_cleanup_applied(state, payload),
        Event::TimerFiredWithToken(payload) => reduce_timer_fired_with_token(state, payload),
        Event::TimerLostWithToken(payload) => reduce_timer_lost_with_token(state, payload),
        Event::EffectFailed(payload) => reduce_effect_failed(state, payload),
    }
}

pub(crate) fn phase0_smoke_fingerprint() -> u64 {
    // Keep phase-0 scaffolding reachable in non-test builds while remaining pure.
    let mut state = CoreState::default();
    let mut fingerprint = 0_u64;
    let animation_token = TimerToken::new(TimerId::Animation, TimerGeneration::new(1));
    let recovery_token = TimerToken::new(TimerId::Recovery, TimerGeneration::new(1));
    let proposal_id = ProposalId::new(1);
    let observed_at = Millis::new(3);
    let demand = ExternalDemand::new(
        crate::core::types::IngressSeq::new(1),
        crate::core::state::ExternalDemandKind::ExternalCursor,
        Millis::new(2),
        None,
    );
    let request = ObservationRequest::new(demand, ProbeRequestSet::default());

    let events = [
        Event::Initialize(InitializeEvent {
            observed_at: Millis::new(1),
        }),
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: crate::core::state::ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(2),
            requested_target: None,
            ingress_cursor_presentation: None,
        }),
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            request: request.clone(),
            basis: crate::core::state::ObservationBasis::new(
                request.observation_id(),
                observed_at,
                "n".to_string(),
                Some(CursorPosition {
                    row: CursorRow(1),
                    col: CursorCol(1),
                }),
                crate::state::CursorLocation::new(1, 1, 1, 1),
                crate::core::types::ViewportSnapshot::new(CursorRow(40), CursorCol(120)),
            ),
            motion: crate::core::state::ObservationMotion::default(),
        }),
        Event::ApplyReported(ApplyReport::AppliedFully {
            proposal_id,
            observed_at: Millis::new(6),
            visual_change: true,
        }),
        Event::ApplyReported(ApplyReport::AppliedDegraded {
            proposal_id,
            divergence: RealizationDivergence::ShellStateUnknown,
            observed_at: Millis::new(7),
            visual_change: true,
        }),
        Event::ApplyReported(ApplyReport::ApplyFailed {
            proposal_id,
            reason: crate::core::state::ApplyFailureKind::ShellError,
            divergence: RealizationDivergence::ShellStateUnknown,
            observed_at: Millis::new(8),
        }),
        Event::TimerFiredWithToken(TimerFiredWithTokenEvent {
            kind: crate::core::effect::TimerKind::Animation,
            token: animation_token,
            observed_at: Millis::new(9),
        }),
        Event::TimerLostWithToken(crate::core::event::TimerLostWithTokenEvent {
            kind: crate::core::effect::TimerKind::Recovery,
            token: recovery_token,
            observed_at: Millis::new(10),
        }),
        Event::EffectFailed(EffectFailedEvent {
            proposal_id: None,
            observed_at: Millis::new(11),
        }),
    ];

    for event in events {
        let transition = reduce(&state, event);
        fingerprint ^= transition.fingerprint();
        state = transition.next;
    }

    fingerprint
        ^ crate::core::types::phase1_types_fingerprint()
        ^ crate::core::state::phase2_probe_fingerprint_seed()
        ^ crate::core::effect::phase4_effect_fingerprint_seed()
}
