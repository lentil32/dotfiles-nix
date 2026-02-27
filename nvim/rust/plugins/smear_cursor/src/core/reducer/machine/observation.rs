use super::Transition;
use super::planning::plan_ready_state;
use super::support::{
    arm_pending_ingress_wake, ingress_cursor_presentation_effect,
    ingress_marks_cursor_autocmd_freshness, invalidate_cleanup_state, next_pending_probe_effect,
    probe_refresh_budget_exhausted_metric, probe_refresh_retry_metric, probe_requests_for,
    record_event_loop_metric, request_observation_base, reset_recovery_attempt,
    suppresses_key_fallback_at,
};
use crate::core::effect::{Effect, EventLoopMetricEffect};
use crate::core::event::{
    ExternalDemandQueuedEvent, InitializeEvent, KeyFallbackQueuedEvent,
    ObservationBaseCollectedEvent, ProbeReportedEvent,
};
use crate::core::state::{
    BackgroundProbeUpdate, CoreState, ExternalDemand, ExternalDemandKind, ObservationRequest,
    ObservationSnapshot, ProbeKind, ProbeReuse, ProbeState, QueuedDemand,
};
use crate::core::types::Millis;

pub(super) fn start_next_observation(
    state: CoreState,
    observed_at: Millis,
) -> (CoreState, Option<Effect>) {
    let mut next_state = state;

    loop {
        let (remaining, next_demand) = next_state.demand_queue().clone().dequeue_ready(observed_at);
        next_state = next_state.with_demand_queue(remaining);

        let Some(demand) = next_demand else {
            return (next_state, None);
        };

        if demand.kind() == ExternalDemandKind::KeyFallback
            && suppresses_key_fallback_at(&next_state, observed_at)
        {
            continue;
        }

        let request = ObservationRequest::new(demand, probe_requests_for(&next_state));
        let remaining = next_state.demand_queue().clone();
        let next_state = next_state.into_observing(request.clone(), remaining);
        let effect = request_observation_base(&next_state, request);
        return (next_state, Some(effect));
    }
}

pub(super) fn transition_ready_or_observe(state: CoreState, observed_at: Millis) -> Transition {
    let (next_state, effect) = start_next_observation(state, observed_at);
    if let Some(effect) = effect {
        return Transition::new(next_state, vec![effect]);
    }

    let settled = match next_state.observation().cloned() {
        Some(observation) => next_state.into_ready_with_observation(observation),
        None => next_state.into_primed(),
    };
    if let Some((scheduled_state, wake)) = arm_pending_ingress_wake(settled.clone(), observed_at) {
        return Transition::new(scheduled_state, vec![wake]);
    }

    Transition::new(settled, Vec::new())
}

pub(super) fn observe_or_plan(state: CoreState, observed_at: Millis) -> Transition {
    let (next_state, effect) = start_next_observation(state, observed_at);
    if let Some(effect) = effect {
        return Transition::new(next_state, vec![effect]);
    }

    let mut transition = match next_state.observation().cloned() {
        Some(observation) => {
            let ready = next_state.into_ready_with_observation(observation.clone());
            plan_ready_state(ready, observation, observed_at)
        }
        None => Transition::new(next_state.into_primed(), Vec::new()),
    };
    if transition.effects.is_empty()
        && matches!(
            transition.next.lifecycle(),
            crate::core::types::Lifecycle::Primed | crate::core::types::Lifecycle::Ready
        )
        && let Some((scheduled_state, wake)) =
            arm_pending_ingress_wake(transition.next.clone(), observed_at)
    {
        transition.next = scheduled_state;
        transition.effects.push(wake);
    }
    transition
}

pub(super) fn reduce_initialize(state: &CoreState, payload: InitializeEvent) -> Transition {
    let _observed_at = payload.observed_at;
    if !state.needs_initialize() {
        return Transition::stay(state);
    }

    Transition::new(state.clone().initialize(), Vec::new())
}

fn queue_external_demand(
    state: CoreState,
    queued_demand: QueuedDemand,
    observed_at: Millis,
) -> Transition {
    let invalidates_cleanup = !matches!(&queued_demand, QueuedDemand::PendingKeyFallback { .. });
    let active_cursor_superseded = state
        .active_demand()
        .is_some_and(|active| active.is_cursor() && queued_demand.is_cursor());
    let (queued, queue_coalesced) = state.demand_queue().clone().enqueue(queued_demand);
    let was_coalesced = active_cursor_superseded || queue_coalesced;
    let queued_state = if invalidates_cleanup {
        invalidate_cleanup_state(state.with_demand_queue(queued))
    } else {
        // Comment: a debounced key-fallback demand is only advisory until it matures into an
        // actual observation. Canceling render cleanup here lets a later-suppressed fallback
        // strand the last smear frame with no retry path.
        state.with_demand_queue(queued)
    };

    let mut transition = match queued_state.protocol() {
        crate::core::state::ProtocolState::Idle { .. } => Transition::stay(&queued_state),
        crate::core::state::ProtocolState::Primed { .. }
        | crate::core::state::ProtocolState::Ready { .. } => {
            transition_ready_or_observe(queued_state, observed_at)
        }
        crate::core::state::ProtocolState::Observing { .. }
        | crate::core::state::ProtocolState::Planning { .. }
        | crate::core::state::ProtocolState::Applying { .. }
        | crate::core::state::ProtocolState::Recovering { .. } => {
            Transition::new(queued_state, Vec::new())
        }
    };

    if was_coalesced {
        transition.effects.insert(
            0,
            record_event_loop_metric(EventLoopMetricEffect::IngressCoalesced),
        );
    }

    transition
}

pub(super) fn reduce_external_demand_queued(
    state: &CoreState,
    payload: ExternalDemandQueuedEvent,
) -> Transition {
    if state.needs_initialize() {
        return Transition::stay(state);
    }

    let state_with_policy = if ingress_marks_cursor_autocmd_freshness(payload.kind) {
        state.clone().with_ingress_policy(
            state
                .ingress_policy()
                .note_cursor_autocmd(payload.observed_at),
        )
    } else {
        state.clone()
    };

    let ingress_effect =
        ingress_cursor_presentation_effect(&state_with_policy, payload.ingress_cursor_presentation);
    let (state_with_seq, seq) = state_with_policy.allocate_ingress_seq();
    let demand = ExternalDemand::new(
        seq,
        payload.kind,
        payload.observed_at,
        payload.requested_target,
    );
    let mut transition = queue_external_demand(
        state_with_seq,
        QueuedDemand::ready(demand),
        payload.observed_at,
    );
    if let Some(effect) = ingress_effect {
        transition.effects.insert(0, effect);
    }
    transition
}

pub(super) fn reduce_key_fallback_queued(
    state: &CoreState,
    payload: KeyFallbackQueuedEvent,
) -> Transition {
    if state.needs_initialize() {
        return Transition::stay(state);
    }

    let (state_with_seq, seq) = state.clone().allocate_ingress_seq();
    let queued_demand = if payload.due_at.value() <= payload.observed_at.value() {
        QueuedDemand::ready(ExternalDemand::new(
            seq,
            ExternalDemandKind::KeyFallback,
            payload.observed_at,
            None,
        ))
    } else {
        QueuedDemand::pending_key_fallback(seq, payload.due_at, None)
    };
    queue_external_demand(state_with_seq, queued_demand, payload.observed_at)
}

pub(super) fn reduce_observation_base_collected(
    state: &CoreState,
    payload: ObservationBaseCollectedEvent,
) -> Transition {
    let Some(active_request) = state.active_observation_request() else {
        return Transition::stay(state);
    };
    if active_request != &payload.request
        || payload.basis.observation_id() != payload.request.observation_id()
    {
        return Transition::stay(state);
    }

    let next_cursor = payload.basis.cursor_position().or(state.last_cursor());
    let next_observation = ObservationSnapshot::new(
        payload.request.clone(),
        payload.basis,
        crate::core::state::ProbeSet::from_request(&payload.request),
        payload.motion,
    );
    let base_state = reset_recovery_attempt(state.clone().with_last_cursor(next_cursor));
    let Some(observing) = base_state.with_active_observation(Some(next_observation.clone())) else {
        return Transition::stay(state);
    };
    let Some(next_probe) = next_pending_probe_effect(&observing, &next_observation) else {
        return complete_observation(observing, next_observation);
    };

    Transition::new(observing, vec![next_probe])
}

enum ProbeReportResolution {
    Updated(Box<ObservationSnapshot>),
    RefreshRequired(ProbeKind),
}

fn apply_probe_report(
    observation: &ObservationSnapshot,
    payload: &ProbeReportedEvent,
) -> Option<ProbeReportResolution> {
    let observation_id = observation.request().observation_id();

    match payload {
        ProbeReportedEvent::CursorColorReady {
            observation_id: reported_id,
            probe_request_id,
            reuse,
            sample,
        } => {
            if *reported_id != observation_id
                || observation.probes().cursor_color().request_id() != Some(*probe_request_id)
                || !observation.probes().cursor_color().is_pending()
            {
                return None;
            }
            if *reuse == ProbeReuse::RefreshRequired {
                return Some(ProbeReportResolution::RefreshRequired(
                    ProbeKind::CursorColor,
                ));
            }
            let probes = observation
                .probes()
                .clone()
                .with_cursor_color(ProbeState::ready(
                    *probe_request_id,
                    *reported_id,
                    *reuse,
                    sample.clone(),
                ));
            Some(ProbeReportResolution::Updated(Box::new(
                observation.clone().with_probes(probes),
            )))
        }
        ProbeReportedEvent::CursorColorFailed {
            observation_id: reported_id,
            probe_request_id,
            failure,
        } => {
            if *reported_id != observation_id
                || observation.probes().cursor_color().request_id() != Some(*probe_request_id)
                || !observation.probes().cursor_color().is_pending()
            {
                return None;
            }
            let probes = observation
                .probes()
                .clone()
                .with_cursor_color(ProbeState::failed(*probe_request_id, *failure));
            Some(ProbeReportResolution::Updated(Box::new(
                observation.clone().with_probes(probes),
            )))
        }
        ProbeReportedEvent::BackgroundReady {
            observation_id: reported_id,
            probe_request_id,
            reuse,
            batch,
        } => {
            if *reported_id != observation_id
                || observation.probes().background().request_id() != Some(*probe_request_id)
                || !observation.probes().background().is_pending()
            {
                return None;
            }
            if *reuse == ProbeReuse::RefreshRequired {
                return Some(ProbeReportResolution::RefreshRequired(
                    ProbeKind::Background,
                ));
            }
            let probes = observation
                .probes()
                .clone()
                .with_background(ProbeState::ready(
                    *probe_request_id,
                    *reported_id,
                    *reuse,
                    batch.clone(),
                ));
            Some(ProbeReportResolution::Updated(Box::new(
                observation
                    .clone()
                    .with_probes(probes)
                    .with_background_progress(None),
            )))
        }
        ProbeReportedEvent::BackgroundChunkReady {
            observation_id: reported_id,
            probe_request_id,
            chunk,
            allowed_mask,
        } => {
            if *reported_id != observation_id
                || observation.probes().background().request_id() != Some(*probe_request_id)
                || !observation.probes().background().is_pending()
            {
                return None;
            }
            let progress = observation.background_progress()?;
            match progress.apply_chunk(*chunk, allowed_mask) {
                Some(BackgroundProbeUpdate::InProgress(next_progress)) => {
                    Some(ProbeReportResolution::Updated(Box::new(
                        observation
                            .clone()
                            .with_background_progress(Some(next_progress)),
                    )))
                }
                Some(BackgroundProbeUpdate::Complete(batch)) => {
                    let probes = observation
                        .probes()
                        .clone()
                        .with_background(ProbeState::ready(
                            *probe_request_id,
                            *reported_id,
                            ProbeReuse::Exact,
                            batch,
                        ));
                    Some(ProbeReportResolution::Updated(Box::new(
                        observation
                            .clone()
                            .with_probes(probes)
                            .with_background_progress(None),
                    )))
                }
                None => None,
            }
        }
        ProbeReportedEvent::BackgroundFailed {
            observation_id: reported_id,
            probe_request_id,
            failure,
        } => {
            if *reported_id != observation_id
                || observation.probes().background().request_id() != Some(*probe_request_id)
                || !observation.probes().background().is_pending()
            {
                return None;
            }
            let probes = observation
                .probes()
                .clone()
                .with_background(ProbeState::failed(*probe_request_id, *failure));
            Some(ProbeReportResolution::Updated(Box::new(
                observation
                    .clone()
                    .with_probes(probes)
                    .with_background_progress(None),
            )))
        }
    }
}

fn complete_observation(state: CoreState, observation: ObservationSnapshot) -> Transition {
    let observed_at = observation.basis().observed_at();
    let next_cursor = observation
        .basis()
        .cursor_position()
        .or(state.last_cursor());
    let ready = reset_recovery_attempt(
        state
            .with_last_cursor(next_cursor)
            .into_ready_with_observation(observation.clone()),
    );
    plan_ready_state(ready, observation, observed_at)
}

pub(super) fn reduce_probe_reported(state: &CoreState, payload: ProbeReportedEvent) -> Transition {
    let Some(active_request) = state.active_observation_request().cloned() else {
        return Transition::stay(state);
    };
    let Some(active_observation) = state.observation().cloned() else {
        return Transition::stay(state);
    };
    let Some(resolution) = apply_probe_report(&active_observation, &payload) else {
        return Transition::stay(state);
    };

    match resolution {
        ProbeReportResolution::RefreshRequired(kind) => {
            // Surprising: the deferred probe no longer matches the captured observation basis.
            // Retry the full base observation instead of fusing mixed-world data.
            let Some(cleared_observation) =
                reset_recovery_attempt(state.clone()).with_active_observation(None)
            else {
                return Transition::stay(state);
            };
            let Some(current_refresh_state) = state.probe_refresh_state() else {
                return Transition::stay(state);
            };
            let (next_refresh_state, exhausted) = current_refresh_state.note_refresh_required(kind);

            if exhausted {
                // Comment: a moving cursor or viewport can keep invalidating one observation.
                // Drop that request after a bounded number of retries so queued ingress regains
                // ownership instead of waiting behind an obsolete observation forever.
                let observed_at = active_request.demand().observed_at();
                let mut transition = observe_or_plan(cleared_observation, observed_at);
                transition
                    .effects
                    .insert(0, probe_refresh_budget_exhausted_metric(kind));
                return transition;
            }

            let Some(retrying) = cleared_observation.with_probe_refresh_state(next_refresh_state)
            else {
                return Transition::stay(state);
            };
            Transition::new(
                retrying.clone(),
                vec![
                    probe_refresh_retry_metric(kind),
                    request_observation_base(&retrying, active_request),
                ],
            )
        }
        ProbeReportResolution::Updated(observation) => {
            let Some(observing) = state
                .clone()
                .with_active_observation(Some((*observation).clone()))
            else {
                return Transition::stay(state);
            };
            if let Some(next_probe) = next_pending_probe_effect(&observing, &observation) {
                return Transition::new(observing, vec![next_probe]);
            }
            complete_observation(observing, *observation)
        }
    }
}
