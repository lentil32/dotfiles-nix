use super::super::super::logging::trace_lazy;
use super::super::super::runtime::EffectExecutor;
use super::super::super::runtime::read_engine_state;
use super::super::super::runtime::record_delayed_ingress_pending_update_count;
use super::super::super::runtime::record_ingress_coalesced_count;
use super::super::super::runtime::record_post_burst_convergence;
use super::super::super::runtime::record_probe_refresh_budget_exhausted_count;
use super::super::super::runtime::record_probe_refresh_retried_count;
use super::super::super::runtime::record_stale_token_event_count;
use super::super::super::trace::effect_summary;
use super::super::labels::core_event_label;
use super::super::labels::effect_label;
use super::dispatch_core_event;
use super::stage_core_event_on_default_queue;
use super::stage_effect_batch_on_default_queue;
use crate::core::effect::Effect;
use crate::core::effect::RequestProbeEffect;
use crate::core::event::Event as CoreEvent;
use crate::core::event::ProbeReportedEvent;
use crate::core::state::ProbeKind;
use crate::core::state::ProbeReuse;
use crate::events::EngineAccessError;
use nvim_oxi::Result;
use std::collections::VecDeque;

use super::queue::EffectOnlyStep;
use super::queue::PendingMetricEffects;

pub(super) fn dispatch_core_event_with_effect_handler<E>(
    initial_event: CoreEvent,
    handle_effects: &mut impl FnMut(Vec<Effect>) -> std::result::Result<(), E>,
) -> std::result::Result<(), E>
where
    E: From<EngineAccessError>,
{
    let event_label = core_event_label(&initial_event);
    let event_summary = super::core_event_summary(&initial_event);
    let (effects, previous_state_summary, next_state_summary) =
        super::mutate_engine_state(|state| {
            let previous_state_summary = super::core_state_summary(state.core_state());
            let transition = super::reduce_core_event_owned(state.take_core_state(), initial_event);
            let next_state_summary = super::core_state_summary(&transition.next);
            let effects = transition.effects;
            state.set_core_state(transition.next);
            (effects, previous_state_summary, next_state_summary)
        })
        .map_err(E::from)?;
    let effect_count = effects.len();
    trace_lazy(|| {
        format!(
            "core_transition event={event_label} details={event_summary} from=[{previous_state_summary}] to=[{next_state_summary}] effects={effect_count}"
        )
    });

    if !effects.is_empty() {
        handle_effects(effects)?;
    }
    Ok(())
}

#[derive(Debug)]
pub(super) struct ScheduledWorkExecutionError {
    pub(super) work_name: &'static str,
    pub(super) error: nvim_oxi::Error,
}

impl From<nvim_oxi::Error> for ScheduledWorkExecutionError {
    fn from(error: nvim_oxi::Error) -> Self {
        Self {
            work_name: "core event dispatch",
            error,
        }
    }
}

impl From<EngineAccessError> for ScheduledWorkExecutionError {
    fn from(error: EngineAccessError) -> Self {
        Self::from(nvim_oxi::Error::from(error))
    }
}

fn execute_pending_metric_effects(metrics: PendingMetricEffects) {
    record_ingress_coalesced_count(metrics.ingress_coalesced);
    record_delayed_ingress_pending_update_count(metrics.delayed_ingress_pending_updated);
    record_stale_token_event_count(metrics.stale_token);
    for (started_at, converged_at) in metrics.cleanup_converged_to_cold {
        record_post_burst_convergence(started_at, converged_at);
    }
    record_probe_refresh_retried_count(
        ProbeKind::CursorColor,
        metrics.cursor_color_probe_refresh_retried,
    );
    record_probe_refresh_retried_count(
        ProbeKind::Background,
        metrics.background_probe_refresh_retried,
    );
    record_probe_refresh_budget_exhausted_count(
        ProbeKind::CursorColor,
        metrics.cursor_color_probe_refresh_budget_exhausted,
    );
    record_probe_refresh_budget_exhausted_count(
        ProbeKind::Background,
        metrics.background_probe_refresh_budget_exhausted,
    );
}

pub(super) fn execute_scheduled_effect_batch(
    effects: Vec<Effect>,
    executor: &mut impl EffectExecutor,
) -> std::result::Result<(), ScheduledWorkExecutionError> {
    let mut follow_ups = VecDeque::new();
    for effect in effects {
        execute_effect_and_collect_follow_ups(effect, executor, false, &mut follow_ups)?;
    }

    if follow_ups.is_empty() {
        return Ok(());
    }

    dispatch_effect_follow_ups(follow_ups, executor)
}

fn execute_single_effect(
    effect: Effect,
    executor: &mut impl EffectExecutor,
) -> std::result::Result<(), ScheduledWorkExecutionError> {
    execute_scheduled_effect_batch(vec![effect], executor)
}

fn execute_effect_and_collect_follow_ups(
    effect: Effect,
    executor: &mut impl EffectExecutor,
    same_reducer_wave_probe: bool,
    follow_ups: &mut VecDeque<CoreEvent>,
) -> std::result::Result<(), ScheduledWorkExecutionError> {
    let effect_name = effect_label(&effect);
    let effect_details = effect_summary(&effect);
    trace_lazy(|| format!("effect_dispatch effect={effect_name} details={effect_details}"));
    let outcome = match effect {
        Effect::RequestProbe(payload) => {
            executor.execute_probe_effect(payload, same_reducer_wave_probe)
        }
        other => executor.execute_effect(other),
    };

    match outcome {
        Ok(new_follow_ups) => {
            trace_lazy(|| {
                format!(
                    "effect_outcome effect={effect_name} details={effect_details} result=ok follow_ups={}",
                    new_follow_ups.len()
                )
            });
            follow_ups.extend(new_follow_ups);
            Ok(())
        }
        Err(err) => {
            trace_lazy(|| {
                format!(
                    "effect_outcome effect={effect_name} details={effect_details} result=err error={err}"
                )
            });
            Err(ScheduledWorkExecutionError {
                work_name: effect_name,
                error: err,
            })
        }
    }
}

fn single_cursor_color_probe_effect(effects: &[Effect]) -> Option<RequestProbeEffect> {
    let [Effect::RequestProbe(payload)] = effects else {
        return None;
    };
    (payload.kind == ProbeKind::CursorColor).then_some(payload.clone())
}

fn execute_same_wave_cursor_color_probe(
    payload: RequestProbeEffect,
    executor: &mut impl EffectExecutor,
) -> std::result::Result<(), ScheduledWorkExecutionError> {
    let mut follow_ups = VecDeque::new();
    execute_effect_and_collect_follow_ups(
        Effect::RequestProbe(payload),
        executor,
        true,
        &mut follow_ups,
    )?;
    dispatch_effect_follow_ups(follow_ups, executor)
}

fn dispatch_follow_up_effects_for_source(
    source_allows_same_wave_probe: bool,
    effects: Vec<Effect>,
    executor: &mut impl EffectExecutor,
) -> std::result::Result<(), ScheduledWorkExecutionError> {
    if source_allows_same_wave_probe
        && let Some(payload) = single_cursor_color_probe_effect(&effects)
    {
        // CONTEXT: a cursor-color probe spawned directly by the freshly collected observation has
        // not crossed a scheduled queue boundary yet, so it can reuse the captured witness
        // exactly once here instead of paying another shell-edge revalidation round-trip.
        return execute_same_wave_cursor_color_probe(payload, executor);
    }

    stage_effect_batch_on_default_queue(effects);
    Ok(())
}

fn dispatch_effect_follow_ups(
    follow_ups: VecDeque<CoreEvent>,
    executor: &mut impl EffectExecutor,
) -> std::result::Result<(), ScheduledWorkExecutionError> {
    for follow_up in follow_ups {
        let work_name = core_event_label(&follow_up);
        if should_schedule_follow_up_event(&follow_up) {
            // retry-class probe reports stay typed reducer inputs, but they hop back onto
            // the scheduled queue so one probe edge cannot immediately replay the next observation.
            stage_core_event_on_default_queue(follow_up);
            continue;
        }

        let source_allows_same_wave_probe = observation_base_allows_same_wave_probe(&follow_up)?;
        let mut handle_effects = |effects| {
            dispatch_follow_up_effects_for_source(source_allows_same_wave_probe, effects, executor)
        };
        if let Err(error) = dispatch_core_event_with_effect_handler(follow_up, &mut handle_effects)
        {
            if error.work_name == "core event dispatch" {
                return Err(ScheduledWorkExecutionError {
                    work_name,
                    error: error.error,
                });
            }
            return Err(error);
        }
    }

    Ok(())
}

fn observation_base_allows_same_wave_probe(
    event: &CoreEvent,
) -> std::result::Result<bool, ScheduledWorkExecutionError> {
    let CoreEvent::ObservationBaseCollected(payload) = event else {
        return Ok(false);
    };

    read_engine_state(|state| {
        state
            .core_state()
            .pending_observation()
            .is_some_and(|pending| {
                pending.observation_id() == payload.observation_id
                    && !pending.requested_probes().background()
            })
    })
    .map_err(ScheduledWorkExecutionError::from)
}

pub(super) fn execute_effect_only_step(
    step: EffectOnlyStep,
    executor: &mut impl EffectExecutor,
) -> std::result::Result<(), ScheduledWorkExecutionError> {
    match step {
        EffectOnlyStep::ApplyRenderCleanup(payload) => {
            execute_single_effect(Effect::ApplyRenderCleanup(payload), executor)
        }
        EffectOnlyStep::ScheduleTimer(payload) => {
            execute_single_effect(Effect::ScheduleTimer(payload), executor)
        }
        EffectOnlyStep::RecordMetrics(metrics) => {
            execute_pending_metric_effects(metrics);
            Ok(())
        }
        EffectOnlyStep::RedrawCmdline => execute_single_effect(Effect::RedrawCmdline, executor),
    }
}

fn should_schedule_follow_up_event(event: &CoreEvent) -> bool {
    matches!(
        event,
        CoreEvent::ProbeReported(
            ProbeReportedEvent::CursorColorReady {
                reuse: ProbeReuse::RefreshRequired,
                ..
            } | ProbeReportedEvent::BackgroundReady {
                reuse: ProbeReuse::RefreshRequired,
                ..
            }
        )
    )
}

pub(super) fn dispatch_scheduled_core_event(event: CoreEvent) -> Result<()> {
    let mut stage_effect_batch = stage_effect_batch_on_default_queue;
    dispatch_core_event(event, &mut stage_effect_batch)
}
