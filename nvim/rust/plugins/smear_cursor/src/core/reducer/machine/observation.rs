use super::Transition;
use super::planning::plan_ready_state;
use super::support::{
    delay_budget_from_ms, ingress_cursor_presentation_effect,
    ingress_marks_cursor_autocmd_freshness, invalidate_cleanup_state, next_pending_probe_effect,
    probe_refresh_budget_exhausted_metric, probe_refresh_retry_metric, probe_requests_for,
    record_event_loop_metric, request_observation_base, reset_recovery_attempt,
    schedule_timer_with_delay,
};
use crate::core::effect::{Effect, EventLoopMetricEffect, TimerKind};
use crate::core::event::{
    ExternalDemandQueuedEvent, InitializeEvent, ObservationBaseCollectedEvent, ProbeReportedEvent,
};
use crate::core::runtime_reducer::as_delay_ms;
use crate::core::state::{
    BackgroundProbeUpdate, CoreState, ExternalDemand, ObservationRequest, ObservationSnapshot,
    ProbeKind, ProbeReuse, ProbeState, QueuedDemand,
};
use crate::core::types::Millis;

pub(super) fn start_next_observation(
    state: CoreState,
    _observed_at: Millis,
) -> (CoreState, Option<Effect>) {
    let (next_state, next_demand) =
        state.map_demand_queue(crate::core::state::DemandQueue::dequeue_ready);
    let Some(demand) = next_demand else {
        return (next_state, None);
    };

    let request = ObservationRequest::new(demand, probe_requests_for(&next_state));
    let next_state = next_state.into_observing(request.clone());
    let effect = request_observation_base(&next_state, request);
    (next_state, Some(effect))
}

pub(super) fn transition_ready_or_observe(state: CoreState, observed_at: Millis) -> Transition {
    let (next_state, effect) = start_next_observation(state, observed_at);
    if let Some(effect) = effect {
        return Transition::new(next_state, vec![effect]);
    }

    let settled = match next_state.retained_observation().cloned() {
        Some(observation) => next_state.into_ready_with_observation(observation),
        None => next_state.into_primed(),
    };

    Transition::new(settled, Vec::new())
}

pub(super) fn observe_or_plan(state: CoreState, observed_at: Millis) -> Transition {
    let (next_state, effect) = start_next_observation(state, observed_at);
    if let Some(effect) = effect {
        return Transition::new(next_state, vec![effect]);
    }

    match next_state.retained_observation().cloned() {
        Some(observation) => {
            let ready = next_state.into_ready_with_observation(observation.clone());
            plan_ready_state(ready, None, observation, observed_at)
        }
        None => Transition::new(next_state.into_primed(), Vec::new()),
    }
}

pub(super) fn reduce_initialize(state: &CoreState, payload: InitializeEvent) -> Transition {
    let _observed_at = payload.observed_at;
    if !state.needs_initialize() {
        return Transition::stay(state);
    }

    Transition::new(state.clone().initialize(), Vec::new())
}

fn delayed_cursor_ingress_transition(state: CoreState, observed_at: Millis) -> Option<Transition> {
    let delay_ms = as_delay_ms(state.runtime().config.delay_event_to_smear);
    if delay_ms == 0 {
        return None;
    }

    let (scheduled_state, effect) = schedule_timer_with_delay(
        state,
        TimerKind::Ingress,
        delay_budget_from_ms(delay_ms),
        observed_at,
    );
    Some(Transition::new(scheduled_state, vec![effect]))
}

fn queue_external_demand(
    state: CoreState,
    queued_demand: QueuedDemand,
    observed_at: Millis,
) -> Transition {
    let should_delay_cursor_ingress = queued_demand.is_cursor();
    let active_cursor_superseded = state
        .active_demand()
        .is_some_and(|active| active.is_cursor() && queued_demand.is_cursor());
    let (queued_state, queue_coalesced) =
        state.map_demand_queue(|queue| queue.enqueue(queued_demand));
    let was_coalesced = active_cursor_superseded || queue_coalesced;
    let queued_state = invalidate_cleanup_state(queued_state);

    let mut transition = match queued_state.protocol() {
        crate::core::state::ProtocolState::Idle { .. } => Transition::stay(&queued_state),
        crate::core::state::ProtocolState::Primed { .. }
        | crate::core::state::ProtocolState::Ready { .. } => {
            if should_delay_cursor_ingress {
                delayed_cursor_ingress_transition(queued_state.clone(), observed_at)
                    .unwrap_or_else(|| transition_ready_or_observe(queued_state, observed_at))
            } else {
                transition_ready_or_observe(queued_state, observed_at)
            }
        }
        crate::core::state::ProtocolState::ObservingRequest { .. }
        | crate::core::state::ProtocolState::ObservingActive { .. }
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
    payload: &ExternalDemandQueuedEvent,
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

    let next_cursor = payload
        .basis
        .cursor_position()
        .or_else(|| state.last_cursor());
    let next_observation = ObservationSnapshot::new(payload.request, payload.basis, payload.motion);
    let base_state = reset_recovery_attempt(state.clone().with_last_cursor(next_cursor));
    let next_probe = next_pending_probe_effect(&base_state, &next_observation);
    let Some(next_probe) = next_probe else {
        return complete_observation(base_state, next_observation);
    };
    let Some(observing) = base_state.with_active_observation(Some(next_observation)) else {
        return Transition::stay(state);
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
            let updated = observation
                .clone()
                .with_cursor_color_probe(ProbeState::ready(
                    *probe_request_id,
                    *reported_id,
                    *reuse,
                    sample.clone(),
                ))?;
            Some(ProbeReportResolution::Updated(Box::new(updated)))
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
            let updated = observation
                .clone()
                .with_cursor_color_probe(ProbeState::failed(*probe_request_id, *failure))?;
            Some(ProbeReportResolution::Updated(Box::new(updated)))
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
            let updated = observation
                .clone()
                .with_background_probe(ProbeState::ready(
                    *probe_request_id,
                    *reported_id,
                    *reuse,
                    batch.clone(),
                ))?;
            Some(ProbeReportResolution::Updated(Box::new(updated)))
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
                    let updated = observation
                        .clone()
                        .with_background_progress(next_progress)?;
                    Some(ProbeReportResolution::Updated(Box::new(updated)))
                }
                Some(BackgroundProbeUpdate::Complete(batch)) => {
                    let updated = observation
                        .clone()
                        .with_background_probe(ProbeState::ready(
                            *probe_request_id,
                            *reported_id,
                            ProbeReuse::Exact,
                            batch,
                        ))?;
                    Some(ProbeReportResolution::Updated(Box::new(updated)))
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
            let updated = observation
                .clone()
                .with_background_probe(ProbeState::failed(*probe_request_id, *failure))?;
            Some(ProbeReportResolution::Updated(Box::new(updated)))
        }
    }
}

fn complete_observation(state: CoreState, observation: ObservationSnapshot) -> Transition {
    let observed_at = observation.basis().observed_at();
    let previous_observation = state.retained_observation().cloned();
    let next_cursor = observation
        .basis()
        .cursor_position()
        .or_else(|| state.last_cursor());
    let ready = reset_recovery_attempt(
        state
            .with_last_cursor(next_cursor)
            .into_ready_with_observation(observation.clone()),
    );
    plan_ready_state(
        ready,
        previous_observation.as_ref(),
        observation,
        observed_at,
    )
}

pub(super) fn reduce_probe_reported(state: &CoreState, payload: &ProbeReportedEvent) -> Transition {
    let Some(active_request) = state.active_observation_request().cloned() else {
        return Transition::stay(state);
    };
    let Some(active_observation) = state.observation().cloned() else {
        return Transition::stay(state);
    };
    let Some(resolution) = apply_probe_report(&active_observation, payload) else {
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
                // a moving cursor or viewport can keep invalidating one observation.
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
            let effect = request_observation_base(&retrying, active_request);
            Transition::new(retrying, vec![probe_refresh_retry_metric(kind), effect])
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
