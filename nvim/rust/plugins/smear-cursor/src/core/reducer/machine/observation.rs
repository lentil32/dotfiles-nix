use super::Transition;
use super::planning::background_probe_plan;
use super::planning::plan_ready_state;
use super::planning::plan_ready_state_with_observation_plan;
use super::planning::prepare_observation_plan;
use super::support::delay_budget_from_ms;
use super::support::enter_hot_cleanup_state;
use super::support::ingress_cursor_presentation_effect;
use super::support::ingress_marks_cursor_autocmd_freshness;
use super::support::next_pending_probe_effect;
use super::support::observation_cursor_color_fallback;
use super::support::probe_refresh_budget_exhausted_metric;
use super::support::probe_refresh_retry_metric;
use super::support::probe_requests_for;
use super::support::record_event_loop_metric;
use super::support::request_observation_base;
use super::support::reset_recovery_attempt;
use super::support::schedule_timer_with_delay;
use crate::core::effect::Effect;
use crate::core::effect::EventLoopMetricEffect;
use crate::core::effect::IngressObservationSurface;
use crate::core::effect::TimerKind;
use crate::core::event::ExternalDemandQueuedEvent;
use crate::core::event::InitializeEvent;
use crate::core::event::ObservationBaseCollectedEvent;
use crate::core::event::ProbeReportedEvent;
use crate::core::runtime_reducer::as_delay_ms;
use crate::core::state::CoreState;
use crate::core::state::ExternalDemand;
use crate::core::state::ObservationSnapshot;
use crate::core::state::PendingObservation;
use crate::core::state::ProbeKind;
use crate::core::state::ProbeReuse;
use crate::core::state::ProbeState;
use crate::core::state::QueuedDemand;
use crate::core::types::IngressSeq;
use crate::core::types::Millis;

#[derive(Debug, Clone, Eq, PartialEq)]
struct ImmediateIngressObservation {
    seq: IngressSeq,
    surface: Option<IngressObservationSurface>,
}

impl ImmediateIngressObservation {
    const fn new(seq: IngressSeq, surface: Option<IngressObservationSurface>) -> Self {
        Self { seq, surface }
    }

    fn surface_for(self, demand: &ExternalDemand) -> Option<IngressObservationSurface> {
        (self.seq == demand.seq()).then_some(self.surface).flatten()
    }
}

fn start_next_observation_with_ingress_surface(
    state: CoreState,
    ingress_observation: Option<ImmediateIngressObservation>,
) -> (CoreState, Option<Effect>) {
    let (next_state, next_demand) =
        state.map_demand_queue(crate::core::state::DemandQueue::dequeue_ready);
    let Some(demand) = next_demand else {
        return (next_state, None);
    };

    let cleared_ingress_policy = next_state.ingress_policy().clear_pending_delay();
    let next_state = next_state.with_ingress_policy(cleared_ingress_policy);
    let buffer_perf_class = demand.buffer_perf_class();
    let pending =
        PendingObservation::new(demand, probe_requests_for(&next_state, buffer_perf_class));
    let ingress_observation_surface =
        ingress_observation.and_then(|observation| observation.surface_for(pending.demand()));
    let Some(next_state) = next_state.enter_observing_request(pending.clone()) else {
        unreachable!("queued observations should only start from primed or ready states");
    };
    let effect = request_observation_base(&next_state, pending, ingress_observation_surface);
    (next_state, Some(effect))
}

pub(super) fn start_next_observation(state: CoreState) -> (CoreState, Option<Effect>) {
    start_next_observation_with_ingress_surface(state, None)
}

pub(super) fn plan_or_stay(state: CoreState, observed_at: Millis) -> Transition {
    match state.lifecycle() {
        crate::core::types::Lifecycle::Ready => plan_ready_state(state, None, observed_at),
        crate::core::types::Lifecycle::Idle
        | crate::core::types::Lifecycle::Primed
        | crate::core::types::Lifecycle::Observing
        | crate::core::types::Lifecycle::Planning
        | crate::core::types::Lifecycle::Applying
        | crate::core::types::Lifecycle::Recovering => Transition::new(state, Vec::new()),
    }
}

pub(super) fn transition_ready_or_observe(state: CoreState) -> Transition {
    transition_ready_or_observe_with_ingress_surface(state, None)
}

fn transition_ready_or_observe_with_ingress_surface(
    state: CoreState,
    ingress_observation: Option<ImmediateIngressObservation>,
) -> Transition {
    debug_assert!(matches!(
        state.lifecycle(),
        crate::core::types::Lifecycle::Primed | crate::core::types::Lifecycle::Ready
    ));

    let (next_state, effect) =
        start_next_observation_with_ingress_surface(state, ingress_observation);
    if let Some(effect) = effect {
        return Transition::new(next_state, vec![effect]);
    }

    Transition::new(next_state, Vec::new())
}

pub(super) fn observe_or_plan(state: CoreState, observed_at: Millis) -> Transition {
    if !matches!(
        state.lifecycle(),
        crate::core::types::Lifecycle::Primed | crate::core::types::Lifecycle::Ready
    ) {
        return Transition::new(state, Vec::new());
    }

    let (next_state, effect) = start_next_observation(state);
    if let Some(effect) = effect {
        return Transition::new(next_state, vec![effect]);
    }

    plan_or_stay(next_state, observed_at)
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
    ingress_observation: Option<ImmediateIngressObservation>,
) -> Transition {
    let should_delay_cursor_ingress = queued_demand.is_cursor();
    let active_cursor_superseded = state
        .active_demand()
        .is_some_and(|active| active.is_cursor() && queued_demand.is_cursor());
    let (queued_state, queue_coalesced) =
        state.map_demand_queue(|queue| queue.enqueue(queued_demand));
    let was_coalesced = active_cursor_superseded || queue_coalesced;
    let (queued_state, hot_cleanup_effects) = enter_hot_cleanup_state(queued_state, observed_at);

    let mut transition = match queued_state.lifecycle() {
        crate::core::types::Lifecycle::Idle => Transition::stay(&queued_state),
        crate::core::types::Lifecycle::Primed | crate::core::types::Lifecycle::Ready => {
            if should_delay_cursor_ingress {
                delayed_cursor_ingress_transition(queued_state.clone(), observed_at).unwrap_or_else(
                    || {
                        transition_ready_or_observe_with_ingress_surface(
                            queued_state,
                            ingress_observation,
                        )
                    },
                )
            } else {
                transition_ready_or_observe_with_ingress_surface(queued_state, ingress_observation)
            }
        }
        crate::core::types::Lifecycle::Observing
        | crate::core::types::Lifecycle::Planning
        | crate::core::types::Lifecycle::Applying
        | crate::core::types::Lifecycle::Recovering => Transition::new(queued_state, Vec::new()),
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
        ingress_observation_surface,
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
    let immediate_ingress_observation =
        ImmediateIngressObservation::new(seq, ingress_observation_surface);
    let mut transition = queue_external_demand(
        state_with_seq,
        QueuedDemand::ready(demand),
        observed_at,
        Some(immediate_ingress_observation),
    );
    if let Some(effect) = ingress_effect {
        transition.effects.insert(0, effect);
    }
    transition
}

pub(super) fn reduce_observation_base_collected(
    mut state: CoreState,
    payload: ObservationBaseCollectedEvent,
) -> Transition {
    let Some(active_pending) = state.pending_observation().cloned() else {
        return Transition::stay_owned(state);
    };
    let ObservationBaseCollectedEvent {
        observation_id,
        basis,
        cursor_color_probe_generations,
        motion,
    } = payload;
    if active_pending.observation_id() != observation_id {
        return Transition::stay_owned(state);
    }
    let pending = active_pending;

    let next_latest_exact_cursor_position =
        state.fallback_cursor_position(motion.exact_cursor_position(basis.cursor_position()));
    let observed_at = basis.observed_at();
    let cursor_color_fallback = observation_cursor_color_fallback(state.retained_observation());
    let previous_observation = state.take_retained_observation();
    let background_requested = pending.requested_probes().background();
    let next_observation = ObservationSnapshot::new(pending, basis, motion)
        .with_cursor_color_probe_generations(cursor_color_probe_generations);
    let (state, prepared_plan) = if background_requested {
        let (state, prepared_plan) = prepare_observation_plan(
            state,
            previous_observation.as_ref(),
            &next_observation,
            observed_at,
        );
        (state, Some(prepared_plan))
    } else {
        (state, None)
    };
    let next_observation = if let Some(prepared_plan) = prepared_plan.as_ref() {
        if let Some(plan) =
            background_probe_plan(prepared_plan, next_observation.basis().viewport())
        {
            let mut next_observation = next_observation;
            *next_observation.probes_mut().background_mut() =
                crate::core::state::BackgroundProbeState::from_plan(plan);
            next_observation
        } else {
            next_observation
        }
    } else {
        next_observation
    };
    let next_observation = complete_mode_scoped_cursor_color_probe(&state, next_observation);
    let mut base_state = reset_recovery_attempt(
        state.with_latest_exact_cursor_position(next_latest_exact_cursor_position),
    );
    let next_probe =
        next_pending_probe_effect(&base_state, &next_observation, cursor_color_fallback);
    let Some(next_probe) = next_probe else {
        if !base_state.enter_ready(next_observation) {
            let _ = base_state.restore_retained_observation(previous_observation);
            return Transition::stay_owned(base_state);
        }
        return finalize_ready_observation(
            base_state,
            prepared_plan,
            previous_observation.as_ref(),
        );
    };
    if !base_state.activate_observation(next_observation) {
        let _ = base_state.restore_retained_observation(previous_observation);
        return Transition::stay_owned(base_state);
    }
    if !base_state.set_prepared_observation_plan(prepared_plan) {
        return Transition::stay_owned(base_state);
    }

    Transition::new(base_state, vec![next_probe])
}

fn complete_mode_scoped_cursor_color_probe(
    state: &CoreState,
    mut observation: ObservationSnapshot,
) -> ObservationSnapshot {
    if !observation.probes().cursor_color().is_requested()
        || state
            .runtime()
            .config
            .requires_cursor_color_sampling_for_mode(observation.basis().mode())
    {
        return observation;
    }

    let _ = observation
        .probes_mut()
        .set_cursor_color_state(ProbeState::ready(ProbeReuse::Exact, None));
    observation
}

enum ProbeReportResolution {
    Updated,
    RefreshRequired(ProbeKind),
}

fn apply_probe_report(
    observation: &mut ObservationSnapshot,
    payload: ProbeReportedEvent,
) -> Option<ProbeReportResolution> {
    let observation_id = observation.observation_id();

    match payload {
        ProbeReportedEvent::CursorColorReady {
            observation_id: reported_id,
            reuse,
            sample,
        } => {
            if reported_id != observation_id || !observation.probes().cursor_color().is_pending() {
                return None;
            }
            if reuse == ProbeReuse::RefreshRequired {
                return Some(ProbeReportResolution::RefreshRequired(
                    ProbeKind::CursorColor,
                ));
            }
            if !observation
                .probes_mut()
                .set_cursor_color_state(ProbeState::ready(reuse, sample))
            {
                return None;
            }
            Some(ProbeReportResolution::Updated)
        }
        ProbeReportedEvent::CursorColorFailed {
            observation_id: reported_id,
            failure,
        } => {
            if reported_id != observation_id || !observation.probes().cursor_color().is_pending() {
                return None;
            }
            if !observation
                .probes_mut()
                .set_cursor_color_state(ProbeState::failed(failure))
            {
                return None;
            }
            Some(ProbeReportResolution::Updated)
        }
        ProbeReportedEvent::BackgroundReady {
            observation_id: reported_id,
            reuse,
            batch,
        } => {
            if reported_id != observation_id {
                return None;
            }
            if reuse == ProbeReuse::RefreshRequired {
                return Some(ProbeReportResolution::RefreshRequired(
                    ProbeKind::Background,
                ));
            }
            let viewport = observation.basis().viewport();
            if !observation
                .probes_mut()
                .background_mut()
                .accept_batch(viewport, reuse, batch)
            {
                return None;
            }
            Some(ProbeReportResolution::Updated)
        }
        ProbeReportedEvent::BackgroundChunkReady {
            observation_id: reported_id,
            chunk,
            allowed_mask,
        } => {
            if reported_id != observation_id {
                return None;
            }
            let viewport = observation.basis().viewport();
            if !observation.probes_mut().background_mut().apply_chunk(
                viewport,
                &chunk,
                &allowed_mask,
            ) {
                return None;
            }
            Some(ProbeReportResolution::Updated)
        }
        ProbeReportedEvent::BackgroundFailed {
            observation_id: reported_id,
            failure,
        } => {
            if reported_id != observation_id {
                return None;
            }
            if !observation
                .probes_mut()
                .background_mut()
                .set_failed(failure)
            {
                return None;
            }
            Some(ProbeReportResolution::Updated)
        }
    }
}

fn finalize_ready_observation(
    mut state: CoreState,
    prepared_plan: Option<crate::core::state::PreparedObservationPlan>,
    previous_observation: Option<&ObservationSnapshot>,
) -> Transition {
    let Some((observed_at, exact_cursor_position)) = state.observation().map(|observation| {
        (
            observation.basis().observed_at(),
            observation.exact_cursor_position(),
        )
    }) else {
        return Transition::stay_owned(state);
    };
    let next_latest_exact_cursor_position = state.fallback_cursor_position(exact_cursor_position);
    let prepared_plan = prepared_plan.or_else(|| state.take_prepared_observation_plan());
    let ready = reset_recovery_attempt(
        state.with_latest_exact_cursor_position(next_latest_exact_cursor_position),
    );
    if let Some(prepared_plan) = prepared_plan {
        plan_ready_state_with_observation_plan(ready, observed_at, prepared_plan)
    } else {
        plan_ready_state(ready, previous_observation, observed_at)
    }
}

pub(super) fn reduce_probe_reported(
    mut state: CoreState,
    payload: ProbeReportedEvent,
) -> Transition {
    if state.observation().is_none() {
        return Transition::stay_owned(state);
    }
    let resolution = {
        let Some(active_observation) = state.observation_mut() else {
            return Transition::stay_owned(state);
        };
        let Some(resolution) = apply_probe_report(active_observation, payload) else {
            return Transition::stay_owned(state);
        };
        resolution
    };

    match resolution {
        ProbeReportResolution::RefreshRequired(kind) => {
            // Surprising: the deferred probe no longer matches the captured observation basis.
            // Retry the full base observation instead of fusing mixed-world data.
            let Some(retry_pending) = state.observation().map(|observation| {
                PendingObservation::new(
                    observation.demand().clone(),
                    probe_requests_for(&state, observation.demand().buffer_perf_class()),
                )
            }) else {
                return Transition::stay_owned(state);
            };
            let mut cleared_observation = reset_recovery_attempt(state);
            if !cleared_observation.replace_active_observation_with_pending(retry_pending) {
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
                let Some(observed_at) = cleared_observation
                    .active_demand()
                    .map(ExternalDemand::observed_at)
                else {
                    return Transition::stay_owned(cleared_observation);
                };
                let mut transition =
                    observe_or_plan(cleared_observation.into_primed(), observed_at);
                transition
                    .effects
                    .insert(0, probe_refresh_budget_exhausted_metric(kind));
                return transition;
            }

            if !cleared_observation.set_probe_refresh_state(next_refresh_state) {
                return Transition::stay_owned(cleared_observation);
            }
            let Some(pending) = cleared_observation.pending_observation().cloned() else {
                return Transition::stay_owned(cleared_observation);
            };
            let effect = request_observation_base(&cleared_observation, pending, None);
            Transition::new(
                cleared_observation,
                vec![probe_refresh_retry_metric(kind), effect],
            )
        }
        ProbeReportResolution::Updated => {
            let next_probe = {
                let Some(observation) = state.observation() else {
                    return Transition::stay_owned(state);
                };
                let cursor_color_fallback = observation_cursor_color_fallback(Some(observation));
                next_pending_probe_effect(&state, observation, cursor_color_fallback)
            };
            if let Some(next_probe) = next_probe {
                return Transition::new(state, vec![next_probe]);
            }
            let prepared_plan = state.take_prepared_observation_plan();
            if !state.complete_active_observation() {
                return Transition::stay_owned(state);
            }
            finalize_ready_observation(state, prepared_plan, None)
        }
    }
}
