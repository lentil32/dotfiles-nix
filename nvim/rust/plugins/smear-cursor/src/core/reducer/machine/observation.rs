use super::Transition;
use super::planning::plan_ready_state;
use super::planning::plan_runtime_transition;
use super::support::delay_budget_from_ms;
use super::support::enter_hot_cleanup_state;
use super::support::ingress_cursor_presentation_effect;
use super::support::ingress_marks_cursor_autocmd_freshness;
use super::support::next_pending_probe_effect;
use super::support::probe_refresh_budget_exhausted_metric;
use super::support::probe_refresh_retry_metric;
use super::support::probe_requests_for;
use super::support::record_event_loop_metric;
use super::support::request_observation_base;
use super::support::reset_recovery_attempt;
use super::support::retained_cursor_color_fallback;
use super::support::schedule_timer_with_delay;
use crate::core::effect::Effect;
use crate::core::effect::EventLoopMetricEffect;
use crate::core::effect::TimerKind;
use crate::core::event::ExternalDemandQueuedEvent;
use crate::core::event::InitializeEvent;
use crate::core::event::ObservationBaseCollectedEvent;
use crate::core::event::ProbeReportedEvent;
use crate::core::runtime_reducer::RenderAction;
use crate::core::runtime_reducer::as_delay_ms;
use crate::core::state::BackgroundProbePlan;
use crate::core::state::BackgroundProbeUpdate;
use crate::core::state::CoreState;
use crate::core::state::ExternalDemand;
use crate::core::state::ObservationRequest;
use crate::core::state::ObservationSnapshot;
use crate::core::state::ProbeKind;
use crate::core::state::ProbeReuse;
use crate::core::state::ProbeState;
use crate::core::state::QueuedDemand;
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

    let cleared_ingress_policy = next_state.ingress_policy().clear_pending_delay();
    let next_state = next_state.with_ingress_policy(cleared_ingress_policy);
    let buffer_perf_class = demand.buffer_perf_class();
    let request =
        ObservationRequest::new(demand, probe_requests_for(&next_state, buffer_perf_class));
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

pub(super) fn reduce_initialize(state: CoreState, payload: InitializeEvent) -> Transition {
    let _observed_at = payload.observed_at;
    if !state.needs_initialize() {
        return Transition::stay_owned(state);
    }

    Transition::new(state.into_primed(), Vec::new())
}

fn delayed_cursor_ingress_transition(state: CoreState, observed_at: Millis) -> Option<Transition> {
    let delay_ms = as_delay_ms(state.runtime().config.delay_event_to_smear);
    if delay_ms == 0 {
        return None;
    }

    let delay_due_at = Millis::new(observed_at.value().saturating_add(delay_ms));
    let was_pending = state.ingress_policy().pending_delay_until().is_some();
    let next_ingress_policy = state
        .ingress_policy()
        .note_pending_delay_until(delay_due_at);
    let next_state = state.with_ingress_policy(next_ingress_policy);
    if was_pending {
        return Some(Transition::new(
            next_state,
            vec![record_event_loop_metric(
                EventLoopMetricEffect::DelayedIngressPendingUpdated,
            )],
        ));
    }

    let (scheduled_state, effect) = schedule_timer_with_delay(
        next_state,
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
    let max_kept_windows = state.runtime().config.max_kept_windows;
    let (queued_state, queue_coalesced) =
        state.map_demand_queue(|queue| queue.enqueue(queued_demand));
    let was_coalesced = active_cursor_superseded || queue_coalesced;
    let (queued_state, hot_cleanup_effects) =
        enter_hot_cleanup_state(queued_state, observed_at, max_kept_windows);

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

    if !hot_cleanup_effects.is_empty() {
        transition.effects.extend(hot_cleanup_effects);
    }

    if was_coalesced {
        transition.effects.insert(
            0,
            record_event_loop_metric(EventLoopMetricEffect::IngressCoalesced),
        );
    }

    transition
}

pub(super) fn reduce_external_demand_queued(
    state: CoreState,
    payload: ExternalDemandQueuedEvent,
) -> Transition {
    if state.needs_initialize() {
        return Transition::stay_owned(state);
    }

    let ExternalDemandQueuedEvent {
        kind,
        observed_at,
        requested_target,
        buffer_perf_class,
        ingress_cursor_presentation,
    } = payload;
    let state_with_policy = if ingress_marks_cursor_autocmd_freshness(kind) {
        let next_ingress_policy = state.ingress_policy().note_cursor_autocmd(observed_at);
        state.with_ingress_policy(next_ingress_policy)
    } else {
        state
    };

    let ingress_effect =
        ingress_cursor_presentation_effect(&state_with_policy, ingress_cursor_presentation);
    let (state_with_seq, seq) = state_with_policy.allocate_ingress_seq();
    let demand = ExternalDemand::new(seq, kind, observed_at, requested_target, buffer_perf_class);
    let mut transition =
        queue_external_demand(state_with_seq, QueuedDemand::ready(demand), observed_at);
    if let Some(effect) = ingress_effect {
        transition.effects.insert(0, effect);
    }
    transition
}

pub(super) fn reduce_observation_base_collected(
    state: CoreState,
    payload: ObservationBaseCollectedEvent,
) -> Transition {
    let Some(active_request) = state.active_observation_request().cloned() else {
        return Transition::stay_owned(state);
    };
    let ObservationBaseCollectedEvent {
        request,
        basis,
        motion,
    } = payload;
    if active_request != request || basis.observation_id() != request.observation_id() {
        return Transition::stay_owned(state);
    }

    let next_cursor = basis.cursor_position().or_else(|| state.last_cursor());
    let observed_at = basis.observed_at();
    let previous_observation = state.retained_observation().cloned();
    let cursor_color_fallback = retained_cursor_color_fallback(previous_observation.as_ref());
    let next_observation = ObservationSnapshot::new(request, basis, motion);
    let next_observation = if let Some(plan) = background_probe_plan_for_observation(
        &state,
        previous_observation.as_ref(),
        &next_observation,
        observed_at,
    ) {
        next_observation.with_background_probe_plan(plan)
    } else {
        next_observation
    };
    let next_observation = complete_mode_scoped_cursor_color_probe(&state, next_observation);
    let mut base_state = reset_recovery_attempt(state.with_last_cursor(next_cursor));
    let next_probe =
        next_pending_probe_effect(&base_state, &next_observation, cursor_color_fallback);
    let Some(next_probe) = next_probe else {
        return complete_observation(base_state, next_observation);
    };
    if !base_state.set_active_observation(Some(next_observation)) {
        return Transition::stay_owned(base_state);
    }

    Transition::new(base_state, vec![next_probe])
}

fn complete_mode_scoped_cursor_color_probe(
    state: &CoreState,
    observation: ObservationSnapshot,
) -> ObservationSnapshot {
    if !observation.request().probes().cursor_color()
        || state
            .runtime()
            .config
            .requires_cursor_color_sampling_for_mode(observation.basis().mode())
    {
        return observation;
    }

    let request_id = ProbeKind::CursorColor.request_id(observation.request().observation_id());
    observation
        .clone()
        .with_cursor_color_probe(ProbeState::ready(
            request_id,
            observation.request().observation_id(),
            ProbeReuse::Exact,
            None,
        ))
        .unwrap_or(observation)
}

fn background_probe_plan_for_observation(
    state: &CoreState,
    previous_observation: Option<&ObservationSnapshot>,
    observation: &ObservationSnapshot,
    observed_at: Millis,
) -> Option<BackgroundProbePlan> {
    if !observation.request().probes().background() {
        return None;
    }

    let (_, cursor_transition) =
        plan_runtime_transition(state, previous_observation, observation, observed_at);
    match cursor_transition.render_decision.render_action {
        RenderAction::Draw(frame) => Some(BackgroundProbePlan::from_render_frame(
            frame.as_ref(),
            observation.basis().viewport(),
        )),
        RenderAction::ClearAll | RenderAction::Noop => None,
    }
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
                    *sample,
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
            match progress.apply_chunk(chunk, allowed_mask) {
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

pub(super) fn reduce_probe_reported(state: CoreState, payload: ProbeReportedEvent) -> Transition {
    let Some(active_request) = state.active_observation_request().cloned() else {
        return Transition::stay_owned(state);
    };
    let Some(active_observation) = state.observation().cloned() else {
        return Transition::stay_owned(state);
    };
    let Some(resolution) = apply_probe_report(&active_observation, &payload) else {
        return Transition::stay_owned(state);
    };

    match resolution {
        ProbeReportResolution::RefreshRequired(kind) => {
            // Surprising: the deferred probe no longer matches the captured observation basis.
            // Retry the full base observation instead of fusing mixed-world data.
            let mut cleared_observation = reset_recovery_attempt(state);
            if !cleared_observation.set_active_observation(None) {
                return Transition::stay_owned(cleared_observation);
            }
            let Some(current_refresh_state) = cleared_observation.probe_refresh_state() else {
                return Transition::stay_owned(cleared_observation);
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

            if !cleared_observation.set_probe_refresh_state(next_refresh_state) {
                return Transition::stay_owned(cleared_observation);
            }
            let effect = request_observation_base(&cleared_observation, active_request);
            Transition::new(
                cleared_observation,
                vec![probe_refresh_retry_metric(kind), effect],
            )
        }
        ProbeReportResolution::Updated(observation) => {
            let mut observing = state;
            if !observing.set_active_observation(Some((*observation).clone())) {
                return Transition::stay_owned(observing);
            }
            let cursor_color_fallback = retained_cursor_color_fallback(Some(&observation));
            if let Some(next_probe) =
                next_pending_probe_effect(&observing, &observation, cursor_color_fallback)
            {
                return Transition::new(observing, vec![next_probe]);
            }
            complete_observation(observing, *observation)
        }
    }
}
