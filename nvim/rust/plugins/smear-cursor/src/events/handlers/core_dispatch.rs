use super::super::logging::trace_lazy;
use super::super::logging::warn;
use super::super::runtime::EffectExecutor;
use super::super::runtime::NeovimEffectExecutor;
use super::super::runtime::mutate_engine_state;
use super::super::runtime::now_ms;
use super::super::runtime::read_engine_state;
use super::super::runtime::record_delayed_ingress_pending_update_count;
use super::super::runtime::record_ingress_coalesced_count;
use super::super::runtime::record_post_burst_convergence;
use super::super::runtime::record_probe_refresh_budget_exhausted_count;
use super::super::runtime::record_probe_refresh_retried_count;
use super::super::runtime::record_scheduled_drain_items;
use super::super::runtime::record_scheduled_drain_items_for_thermal;
use super::super::runtime::record_scheduled_drain_reschedule;
use super::super::runtime::record_scheduled_drain_reschedule_for_thermal;
use super::super::runtime::record_scheduled_queue_depth;
use super::super::runtime::record_scheduled_queue_depth_for_thermal;
use super::super::runtime::record_stale_token_event_count;
use super::super::runtime::to_core_millis;
use super::super::timers::schedule_guarded;
use super::super::trace::core_event_summary;
use super::super::trace::core_state_summary;
use super::super::trace::effect_summary;
use super::labels::core_event_label;
use super::labels::effect_label;
use crate::core::effect::ApplyRenderCleanupEffect;
use crate::core::effect::Effect;
use crate::core::effect::EventLoopMetricEffect;
use crate::core::effect::RequestProbeEffect;
use crate::core::effect::ScheduleTimerEffect;
use crate::core::effect::TimerKind;
use crate::core::event::EffectFailedEvent;
use crate::core::event::Event as CoreEvent;
use crate::core::event::ProbeReportedEvent;
use crate::core::reducer::reduce_owned as reduce_core_event_owned;
use crate::core::state::ProbeKind;
use crate::core::state::ProbeReuse;
use crate::core::state::RenderThermalState;
use crate::core::types::Millis;
use nvim_oxi::Result;
use std::cell::RefCell;
use std::collections::VecDeque;

const MIN_SCHEDULED_WORK_ITEMS_PER_EDGE: usize = 16;
const MAX_SCHEDULED_WORK_ITEMS_PER_EDGE: usize = 32;
const MAX_CHAINED_SCHEDULED_WORK_ITEMS_PER_EDGE: usize = 96;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ScheduledEffectDrainEntry {
    NextItem,
}

impl ScheduledEffectDrainEntry {
    const fn context(self) -> &'static str {
        match self {
            Self::NextItem => "core effect drain",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ScheduledWorkItem {
    EffectBatch(Vec<Effect>),
    CoreEvent(Box<CoreEvent>),
    EffectOnlyAgenda(EffectOnlyAgenda),
}

#[derive(Debug, Clone, PartialEq)]
enum ScheduledWorkUnit {
    EffectBatch(Vec<Effect>),
    CoreEvent(Box<CoreEvent>),
    EffectOnlyStep(EffectOnlyStep),
}

#[derive(Debug, Clone, Default, PartialEq)]
struct EffectOnlyAgenda {
    steps: VecDeque<EffectOnlyStep>,
}

impl EffectOnlyAgenda {
    fn append_effects(&mut self, effects: Vec<Effect>) -> usize {
        effects
            .into_iter()
            .map(|effect| usize::from(self.append_effect(effect)))
            .sum()
    }

    fn append_effect(&mut self, effect: Effect) -> bool {
        match effect {
            Effect::ApplyRenderCleanup(payload) => {
                if matches!(
                    self.steps.back(),
                    Some(EffectOnlyStep::ApplyRenderCleanup(existing)) if *existing == payload
                ) {
                    return false;
                }
                self.steps
                    .push_back(EffectOnlyStep::ApplyRenderCleanup(payload));
                true
            }
            Effect::ScheduleTimer(payload) => {
                if let Some(EffectOnlyStep::ScheduleTimer(existing)) = self.steps.back_mut()
                    && TimerKind::from_timer_id(existing.token.id())
                        == TimerKind::from_timer_id(payload.token.id())
                {
                    *existing = payload;
                    return false;
                }
                self.steps.push_back(EffectOnlyStep::ScheduleTimer(payload));
                true
            }
            Effect::RecordEventLoopMetric(metric) => {
                if let Some(EffectOnlyStep::RecordMetrics(metrics)) = self.steps.back_mut() {
                    metrics.record(metric);
                    return false;
                }
                let mut metrics = PendingMetricEffects::default();
                metrics.record(metric);
                self.steps.push_back(EffectOnlyStep::RecordMetrics(metrics));
                true
            }
            Effect::RedrawCmdline => {
                if matches!(self.steps.back(), Some(EffectOnlyStep::RedrawCmdline)) {
                    return false;
                }
                self.steps.push_back(EffectOnlyStep::RedrawCmdline);
                true
            }
            _ => unreachable!("only effect-only agenda effects should be appended"),
        }
    }

    fn pop_step(&mut self) -> Option<EffectOnlyStep> {
        self.steps.pop_front()
    }

    fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
enum EffectOnlyStep {
    ApplyRenderCleanup(ApplyRenderCleanupEffect),
    ScheduleTimer(ScheduleTimerEffect),
    RecordMetrics(PendingMetricEffects),
    RedrawCmdline,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct PendingMetricEffects {
    ingress_coalesced: usize,
    delayed_ingress_pending_updated: usize,
    stale_token: usize,
    cleanup_converged_to_cold: Vec<(Millis, Millis)>,
    cursor_color_probe_refresh_retried: usize,
    background_probe_refresh_retried: usize,
    cursor_color_probe_refresh_budget_exhausted: usize,
    background_probe_refresh_budget_exhausted: usize,
}

impl PendingMetricEffects {
    fn record(&mut self, metric: EventLoopMetricEffect) {
        match metric {
            EventLoopMetricEffect::IngressCoalesced => {
                self.ingress_coalesced = self.ingress_coalesced.saturating_add(1);
            }
            EventLoopMetricEffect::DelayedIngressPendingUpdated => {
                self.delayed_ingress_pending_updated =
                    self.delayed_ingress_pending_updated.saturating_add(1);
            }
            EventLoopMetricEffect::CleanupConvergedToCold {
                started_at,
                converged_at,
            } => {
                self.cleanup_converged_to_cold
                    .push((started_at, converged_at));
            }
            EventLoopMetricEffect::StaleToken => {
                self.stale_token = self.stale_token.saturating_add(1);
            }
            EventLoopMetricEffect::ProbeRefreshRetried(kind) => {
                *self.refresh_retried_count_mut(kind) =
                    self.refresh_retried_count(kind).saturating_add(1);
            }
            EventLoopMetricEffect::ProbeRefreshBudgetExhausted(kind) => {
                *self.refresh_budget_exhausted_count_mut(kind) =
                    self.refresh_budget_exhausted_count(kind).saturating_add(1);
            }
        }
    }

    fn refresh_retried_count(&self, kind: ProbeKind) -> usize {
        match kind {
            ProbeKind::CursorColor => self.cursor_color_probe_refresh_retried,
            ProbeKind::Background => self.background_probe_refresh_retried,
        }
    }

    fn refresh_retried_count_mut(&mut self, kind: ProbeKind) -> &mut usize {
        match kind {
            ProbeKind::CursorColor => &mut self.cursor_color_probe_refresh_retried,
            ProbeKind::Background => &mut self.background_probe_refresh_retried,
        }
    }

    fn refresh_budget_exhausted_count(&self, kind: ProbeKind) -> usize {
        match kind {
            ProbeKind::CursorColor => self.cursor_color_probe_refresh_budget_exhausted,
            ProbeKind::Background => self.background_probe_refresh_budget_exhausted,
        }
    }

    fn refresh_budget_exhausted_count_mut(&mut self, kind: ProbeKind) -> &mut usize {
        match kind {
            ProbeKind::CursorColor => &mut self.cursor_color_probe_refresh_budget_exhausted,
            ProbeKind::Background => &mut self.background_probe_refresh_budget_exhausted,
        }
    }
}

#[derive(Default)]
struct ScheduledEffectQueueState {
    items: VecDeque<ScheduledWorkItem>,
    pending_work_units: usize,
    drain_scheduled: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct ScheduledStageResult {
    should_schedule: bool,
    depth: usize,
}

impl ScheduledEffectQueueState {
    fn stage_irreducible_item(&mut self, item: ScheduledWorkItem) -> ScheduledStageResult {
        self.items.push_back(item);
        self.pending_work_units = self.pending_work_units.saturating_add(1);
        self.finish_stage()
    }

    fn finish_stage(&mut self) -> ScheduledStageResult {
        let depth = self.pending_work_units;
        let should_schedule = if self.drain_scheduled {
            false
        } else {
            self.drain_scheduled = true;
            true
        };
        ScheduledStageResult {
            should_schedule,
            depth,
        }
    }

    fn stage_batch(&mut self, effects: Vec<Effect>) -> ScheduledStageResult {
        if effects.is_empty() {
            return ScheduledStageResult {
                should_schedule: false,
                depth: self.pending_work_units,
            };
        }

        if !effects.iter().all(is_continuable_hot_follow_up_effect) {
            return self.stage_irreducible_item(ScheduledWorkItem::EffectBatch(effects));
        }

        // CONTEXT: Adjacent runtime-only cleanup/timer/telemetry waves dominate queue churn in hot
        // bursts. Fold them into a typed agenda so repeated reschedules collapse without moving
        // reducer work across an irreducible queue boundary.
        let added_work_units = match self.items.back_mut() {
            Some(ScheduledWorkItem::EffectOnlyAgenda(agenda)) => agenda.append_effects(effects),
            _ => {
                let mut agenda = EffectOnlyAgenda::default();
                let added_work_units = agenda.append_effects(effects);
                self.items
                    .push_back(ScheduledWorkItem::EffectOnlyAgenda(agenda));
                added_work_units
            }
        };
        self.pending_work_units = self.pending_work_units.saturating_add(added_work_units);
        self.finish_stage()
    }

    fn stage_core_event(&mut self, event: CoreEvent) -> ScheduledStageResult {
        self.stage_irreducible_item(ScheduledWorkItem::CoreEvent(Box::new(event)))
    }

    fn pop_work_unit(&mut self) -> Option<ScheduledWorkUnit> {
        let unit = match self.items.front()? {
            ScheduledWorkItem::EffectBatch(_) => match self.items.pop_front()? {
                ScheduledWorkItem::EffectBatch(effects) => ScheduledWorkUnit::EffectBatch(effects),
                _ => unreachable!("front queue item should stay stable while borrowed"),
            },
            ScheduledWorkItem::CoreEvent(_) => match self.items.pop_front()? {
                ScheduledWorkItem::CoreEvent(event) => ScheduledWorkUnit::CoreEvent(event),
                _ => unreachable!("front queue item should stay stable while borrowed"),
            },
            ScheduledWorkItem::EffectOnlyAgenda(_) => {
                let (step, agenda_is_empty) = {
                    match self.items.front_mut() {
                        Some(ScheduledWorkItem::EffectOnlyAgenda(agenda)) => {
                            let Some(step) = agenda.pop_step() else {
                                unreachable!("non-empty effect-only agenda should yield a step");
                            };
                            (step, agenda.is_empty())
                        }
                        _ => unreachable!("front queue item should stay stable while borrowed"),
                    }
                };
                if agenda_is_empty {
                    let _ = self.items.pop_front();
                }
                ScheduledWorkUnit::EffectOnlyStep(step)
            }
        };
        self.pending_work_units = self.pending_work_units.saturating_sub(1);
        Some(unit)
    }

    fn reset(&mut self) {
        self.items.clear();
        self.pending_work_units = 0;
        self.drain_scheduled = false;
    }
}

thread_local! {
    static SCHEDULED_EFFECT_QUEUE: RefCell<ScheduledEffectQueueState> =
        RefCell::new(ScheduledEffectQueueState::default());
}

fn with_scheduled_effect_queue<R>(mutator: impl FnOnce(&mut ScheduledEffectQueueState) -> R) -> R {
    SCHEDULED_EFFECT_QUEUE.with(|queue| {
        // Keep queue borrows scoped to staging/pop bookkeeping only. Reducer execution and effect
        // dispatch always happen after this borrow is released, so re-entering here would signal a
        // structural bug we should fix directly instead of silently dropping queued work.
        let mut queue = queue.borrow_mut();
        mutator(&mut queue)
    })
}

fn schedule_scheduled_effect_drain(entrypoint: ScheduledEffectDrainEntry) {
    schedule_guarded(entrypoint.context(), move || {
        run_scheduled_effect_drain(entrypoint);
    });
}

fn current_cleanup_thermal_state() -> Option<RenderThermalState> {
    read_engine_state(|state| state.core_state().render_cleanup().thermal()).ok()
}

fn stage_effect_batch_on_default_queue(effects: Vec<Effect>) {
    if effects.is_empty() {
        return;
    }

    let stage = with_scheduled_effect_queue(|queue| queue.stage_batch(effects));
    record_scheduled_queue_depth(stage.depth);
    if let Some(thermal) = current_cleanup_thermal_state() {
        record_scheduled_queue_depth_for_thermal(thermal, stage.depth);
    }
    if stage.should_schedule {
        schedule_scheduled_effect_drain(ScheduledEffectDrainEntry::NextItem);
    }
}

fn stage_core_event_on_default_queue(event: CoreEvent) {
    let stage = with_scheduled_effect_queue(|queue| queue.stage_core_event(event));
    record_scheduled_queue_depth(stage.depth);
    if let Some(thermal) = current_cleanup_thermal_state() {
        record_scheduled_queue_depth_for_thermal(thermal, stage.depth);
    }
    if stage.should_schedule {
        schedule_scheduled_effect_drain(ScheduledEffectDrainEntry::NextItem);
    }
}

fn dispatch_core_event_with_effect_handler<E>(
    initial_event: CoreEvent,
    handle_effects: &mut impl FnMut(Vec<Effect>) -> std::result::Result<(), E>,
) -> std::result::Result<(), E>
where
    E: From<crate::events::EngineAccessError>,
{
    let event_label = core_event_label(&initial_event);
    let event_summary = core_event_summary(&initial_event);
    let (effects, previous_state_summary, next_state_summary) = mutate_engine_state(|state| {
        let previous_state_summary = core_state_summary(state.core_state());
        let transition = reduce_core_event_owned(state.take_core_state(), initial_event);
        let next_state_summary = core_state_summary(&transition.next);
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

pub(crate) fn dispatch_core_event(
    initial_event: CoreEvent,
    stage_effect_batch: &mut impl FnMut(Vec<Effect>),
) -> Result<()> {
    let mut handle_effects = |effects| {
        stage_effect_batch(effects);
        Ok(())
    };
    dispatch_core_event_with_effect_handler(initial_event, &mut handle_effects)
}

pub(crate) fn dispatch_core_events(
    initial_events: impl IntoIterator<Item = CoreEvent>,
    stage_effect_batch: &mut impl FnMut(Vec<Effect>),
) -> Result<()> {
    for event in initial_events {
        dispatch_core_event(event, stage_effect_batch)?;
    }
    Ok(())
}

pub(crate) fn dispatch_core_event_with_default_scheduler(initial_event: CoreEvent) -> Result<()> {
    dispatch_core_events_with_default_scheduler([initial_event])
}

pub(crate) fn dispatch_core_events_with_default_scheduler(
    initial_events: impl IntoIterator<Item = CoreEvent>,
) -> Result<()> {
    let mut stage_effect_batch = stage_effect_batch_on_default_queue;
    dispatch_core_events(initial_events, &mut stage_effect_batch)
}

pub(crate) fn stage_core_event_with_default_scheduler(initial_event: CoreEvent) {
    stage_core_event_on_default_queue(initial_event);
}

pub(crate) fn reset_scheduled_effect_queue() {
    with_scheduled_effect_queue(ScheduledEffectQueueState::reset);
}

#[derive(Debug)]
struct ScheduledWorkExecutionError {
    work_name: &'static str,
    error: nvim_oxi::Error,
}

impl From<nvim_oxi::Error> for ScheduledWorkExecutionError {
    fn from(error: nvim_oxi::Error) -> Self {
        Self {
            work_name: "core event dispatch",
            error,
        }
    }
}

impl From<crate::events::EngineAccessError> for ScheduledWorkExecutionError {
    fn from(error: crate::events::EngineAccessError) -> Self {
        Self::from(nvim_oxi::Error::from(error))
    }
}

fn handle_scheduled_work_drain_failure(work_name: &'static str, error: &nvim_oxi::Error) {
    reset_scheduled_queue_after_failure();
    warn(&format!("scheduled core work failed: {work_name}: {error}"));
    let observed_at = to_core_millis(now_ms());
    stage_core_event_with_default_scheduler(CoreEvent::EffectFailed(EffectFailedEvent {
        proposal_id: None,
        observed_at,
    }));
}

fn reset_scheduled_queue_after_failure() {
    reset_scheduled_effect_queue();
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

fn execute_scheduled_effect_batch(
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

        let source_allows_same_wave_probe = matches!(
            follow_up,
            CoreEvent::ObservationBaseCollected(ref payload) if !payload.request.probes().background()
        );
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

fn execute_effect_only_step(
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

fn dispatch_scheduled_core_event(event: CoreEvent) -> Result<()> {
    let mut stage_effect_batch = stage_effect_batch_on_default_queue;
    dispatch_core_event(event, &mut stage_effect_batch)
}

fn scheduled_drain_budget() -> usize {
    let thermal = current_cleanup_thermal_state().unwrap_or(RenderThermalState::Cold);
    with_scheduled_effect_queue(|queue| {
        let queued_work_units = queue.pending_work_units;
        if queued_work_units == 0 {
            queue.drain_scheduled = false;
            return 0;
        }

        if thermal == RenderThermalState::Hot && queue_has_only_effect_only_agendas(queue) {
            return scheduled_drain_budget_for_hot_effect_only_snapshot(queued_work_units);
        }

        scheduled_drain_budget_for_thermal(thermal, queued_work_units)
    })
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct ScheduledDrainSnapshot {
    thermal: RenderThermalState,
    queued_work_units: usize,
    budget: usize,
}

fn scheduled_drain_snapshot() -> ScheduledDrainSnapshot {
    let thermal = current_cleanup_thermal_state().unwrap_or(RenderThermalState::Cold);
    let queued_work_units = with_scheduled_effect_queue(|queue| queue.pending_work_units);
    ScheduledDrainSnapshot {
        thermal,
        queued_work_units,
        budget: scheduled_drain_budget(),
    }
}

fn queue_has_only_effect_only_agendas(queue: &ScheduledEffectQueueState) -> bool {
    !queue.items.is_empty()
        && queue
            .items
            .iter()
            .all(|item| matches!(item, ScheduledWorkItem::EffectOnlyAgenda(_)))
}

fn scheduled_drain_budget_for_thermal(
    thermal: RenderThermalState,
    queued_work_units: usize,
) -> usize {
    match thermal {
        RenderThermalState::Cooling => {
            // Surprising: Cooling still drains only the snapshot that was already queued when this
            // edge started. Refresh-required probe retries must remain deferred to the next edge,
            // so aggressive convergence expands the snapshot budget instead of recursively draining
            // follow-up work staged mid-pass.
            queued_work_units
        }
        RenderThermalState::Hot | RenderThermalState::Cold => {
            scheduled_drain_budget_for_depth(queued_work_units)
        }
    }
}

fn scheduled_drain_budget_for_hot_effect_only_snapshot(queued_work_units: usize) -> usize {
    if queued_work_units == 0 {
        return 0;
    }

    // CONTEXT: once Hot queue state has collapsed down to effect-only cleanup/timer/metric/redraw
    // steps, draining only half the backlog turns one reducer burst into repeated shell edges even
    // though the queue no longer carries irreducible reducer work. Drain that typed snapshot in
    // one callback, but keep the existing chained-edge ceiling so a pathological wave still yields
    // back to the shell.
    queued_work_units.min(MAX_CHAINED_SCHEDULED_WORK_ITEMS_PER_EDGE)
}

fn scheduled_drain_budget_for_depth(queued_work_units: usize) -> usize {
    if queued_work_units == 0 {
        return 0;
    }

    // Drain a bounded fraction of the queued snapshot so backlog converges geometrically under
    // burst load, while smaller queues still clear in a single shell edge.
    let half_backlog = queued_work_units.saturating_add(1) / 2;
    let bounded_half = half_backlog.clamp(
        MIN_SCHEDULED_WORK_ITEMS_PER_EDGE,
        MAX_SCHEDULED_WORK_ITEMS_PER_EDGE,
    );
    queued_work_units.min(bounded_half)
}

fn should_continue_effect_only_follow_up_chain(
    snapshot: ScheduledDrainSnapshot,
    drained_items_this_pass: usize,
    drained_items_total: usize,
) -> bool {
    if snapshot.thermal != RenderThermalState::Hot
        || drained_items_this_pass != snapshot.budget
        || drained_items_total >= MAX_CHAINED_SCHEDULED_WORK_ITEMS_PER_EDGE
    {
        return false;
    }

    with_scheduled_effect_queue(|queue| {
        // Mixed hot snapshots often drain their irreducible reducer work first and leave only
        // cleanup/timer/metric/redraw tail work queued. Once that remaining queue has collapsed
        // to effect-only agendas, continue immediately in the same callback instead of paying an
        // extra scheduled shell edge for the tail.
        !queue.items.is_empty()
            && queue
                .items
                .iter()
                .all(|item| matches!(item, ScheduledWorkItem::EffectOnlyAgenda(_)))
    })
}

fn is_continuable_hot_follow_up_effect(effect: &Effect) -> bool {
    matches!(
        effect,
        Effect::ApplyRenderCleanup(_)
            | Effect::ScheduleTimer(_)
            | Effect::RecordEventLoopMetric(_)
            | Effect::RedrawCmdline
    )
}

fn drain_scheduled_work_with_executor(
    executor: &mut impl EffectExecutor,
) -> std::result::Result<bool, ScheduledWorkExecutionError> {
    // Drain a bounded snapshot of already-staged work so backlog can recover faster, while any
    // reducer follow-ups created during this pass still remain deferred to a later scheduled edge.
    let drain_thermal = current_cleanup_thermal_state();
    let mut drained_items = 0_usize;
    loop {
        let snapshot = scheduled_drain_snapshot();
        let mut remaining_budget = snapshot.budget;
        let mut drained_items_this_pass = 0_usize;
        while remaining_budget > 0 {
            let Some(item) = with_scheduled_effect_queue(ScheduledEffectQueueState::pop_work_unit)
            else {
                with_scheduled_effect_queue(|queue| {
                    queue.drain_scheduled = false;
                });
                if drained_items > 0 {
                    record_scheduled_drain_items(drained_items);
                    if let Some(thermal) = drain_thermal {
                        record_scheduled_drain_items_for_thermal(thermal, drained_items);
                    }
                }
                return Ok(false);
            };

            match item {
                ScheduledWorkUnit::EffectBatch(effects) => {
                    execute_scheduled_effect_batch(effects, executor)?;
                }
                ScheduledWorkUnit::CoreEvent(event) => {
                    let work_name = core_event_label(&event);
                    dispatch_scheduled_core_event(*event)
                        .map_err(|error| ScheduledWorkExecutionError { work_name, error })?;
                }
                ScheduledWorkUnit::EffectOnlyStep(step) => {
                    execute_effect_only_step(step, executor)?;
                }
            }
            drained_items = drained_items.saturating_add(1);
            drained_items_this_pass = drained_items_this_pass.saturating_add(1);
            remaining_budget -= 1;
        }

        // Once Hot drains the full pre-existing snapshot, any remaining cleanup/timer batches were
        // spawned mid-pass by reducer follow-ups inside this callback. Continue through a bounded
        // number of those low-risk waves here so immediate cleanup convergence does not pay an
        // extra schedule edge per reducer hop. Observation batches and queued probe batches still
        // remain deferred; only the single cursor-color probe attached directly to an
        // ObservationBaseCollected follow-up can execute inline before that queue hop exists.
        if should_continue_effect_only_follow_up_chain(
            snapshot,
            drained_items_this_pass,
            drained_items,
        ) {
            continue;
        }

        break;
    }

    let has_more_items = with_scheduled_effect_queue(|queue| {
        let has_more_items = !queue.items.is_empty();
        if !has_more_items {
            queue.drain_scheduled = false;
        }
        has_more_items
    });

    if drained_items > 0 {
        record_scheduled_drain_items(drained_items);
        if let Some(thermal) = drain_thermal {
            record_scheduled_drain_items_for_thermal(thermal, drained_items);
        }
    }
    if has_more_items {
        record_scheduled_drain_reschedule();
        if let Some(thermal) = drain_thermal {
            record_scheduled_drain_reschedule_for_thermal(thermal);
        }
    }

    Ok(has_more_items)
}

fn run_scheduled_effect_drain(entrypoint: ScheduledEffectDrainEntry) {
    let mut executor = match NeovimEffectExecutor::new() {
        Ok(executor) => executor,
        Err(err) => {
            handle_scheduled_work_drain_failure(entrypoint.context(), &err);
            return;
        }
    };
    let drain_outcome = match entrypoint {
        ScheduledEffectDrainEntry::NextItem => drain_scheduled_work_with_executor(&mut executor),
    };

    match drain_outcome {
        Ok(true) => schedule_scheduled_effect_drain(entrypoint),
        Ok(false) => {}
        Err(err) => {
            handle_scheduled_work_drain_failure(err.work_name, &err.error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ScheduledWorkItem;
    use super::ScheduledWorkUnit;
    use super::dispatch_core_event;
    use super::drain_scheduled_work_with_executor;
    use super::reset_scheduled_effect_queue;
    use super::reset_scheduled_queue_after_failure;
    use super::scheduled_drain_budget;
    use super::scheduled_drain_budget_for_depth;
    use super::scheduled_drain_budget_for_hot_effect_only_snapshot;
    use super::scheduled_drain_budget_for_thermal;
    use super::with_scheduled_effect_queue;
    use crate::core::effect::Effect;
    use crate::core::effect::EventLoopMetricEffect;
    use crate::core::effect::IngressCursorPresentationEffect;
    use crate::core::effect::RequestProbeEffect;
    use crate::core::effect::ScheduleTimerEffect;
    use crate::core::event::Event as CoreEvent;
    use crate::core::event::ExternalDemandQueuedEvent;
    use crate::core::event::ObservationBaseCollectedEvent;
    use crate::core::event::ProbeReportedEvent;
    use crate::core::reducer::reduce as reduce_core_event;
    use crate::core::state::BackgroundProbeBatch;
    use crate::core::state::BackgroundProbeChunk;
    use crate::core::state::BackgroundProbeChunkMask;
    use crate::core::state::BackgroundProbePlan;
    use crate::core::state::BufferPerfClass;
    use crate::core::state::CoreState;
    use crate::core::state::CursorColorSample;
    use crate::core::state::ExternalDemandKind;
    use crate::core::state::ObservationBasis;
    use crate::core::state::ObservationMotion;
    use crate::core::state::ObservationRequest;
    use crate::core::state::ObservationSnapshot;
    use crate::core::state::ProbeKind;
    use crate::core::state::ProbeReuse;
    use crate::core::state::RenderCleanupState;
    use crate::core::state::RenderThermalState;
    use crate::core::types::CursorCol;
    use crate::core::types::CursorPosition;
    use crate::core::types::CursorRow;
    use crate::core::types::DelayBudgetMs;
    use crate::core::types::Lifecycle;
    use crate::core::types::Millis;
    use crate::core::types::TimerGeneration;
    use crate::core::types::TimerId;
    use crate::core::types::TimerToken;
    use crate::core::types::ViewportSnapshot;
    use crate::events::runtime::core_state;
    use crate::events::runtime::set_core_state;
    use crate::mutex::lock_with_poison_recovery;
    use crate::state::CursorLocation;
    use crate::types::ScreenCell;
    use nvim_oxi::Result;
    use std::collections::VecDeque;
    use std::sync::LazyLock;
    use std::sync::Mutex;

    static CORE_DISPATCH_TEST_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn core_dispatch_test_guard() -> std::sync::MutexGuard<'static, ()> {
        lock_with_poison_recovery(&CORE_DISPATCH_TEST_MUTEX, |_| (), |_| {})
    }

    #[derive(Default)]
    struct RecordingExecutor {
        executed_effects: Vec<Effect>,
        planned_follow_ups: VecDeque<Vec<CoreEvent>>,
    }

    impl super::EffectExecutor for RecordingExecutor {
        fn execute_effect(&mut self, effect: Effect) -> Result<Vec<CoreEvent>> {
            self.executed_effects.push(effect);
            Ok(self.planned_follow_ups.pop_front().unwrap_or_default())
        }
    }

    struct FailingExecutor;

    impl super::EffectExecutor for FailingExecutor {
        fn execute_effect(&mut self, _effect: Effect) -> Result<Vec<CoreEvent>> {
            Err(nvim_oxi::api::Error::Other("planned scheduled drain failure".to_string()).into())
        }
    }

    fn ready_state() -> CoreState {
        let mut runtime = crate::state::RuntimeState::default();
        runtime.config.delay_event_to_smear = 0.0;
        CoreState::default().with_runtime(runtime).initialize()
    }

    fn cursor(row: u32, col: u32) -> CursorPosition {
        CursorPosition {
            row: CursorRow(row),
            col: CursorCol(col),
        }
    }

    fn observation_basis(
        request: &ObservationRequest,
        position: Option<CursorPosition>,
        observed_at: u64,
    ) -> ObservationBasis {
        ObservationBasis::new(
            request.observation_id(),
            Millis::new(observed_at),
            "n".to_string(),
            position,
            CursorLocation::new(11, 22, 3, 4),
            ViewportSnapshot::new(CursorRow(40), CursorCol(120)),
        )
    }

    fn refresh_required_probe_report(request: &ObservationRequest) -> CoreEvent {
        CoreEvent::ProbeReported(ProbeReportedEvent::CursorColorReady {
            observation_id: request.observation_id(),
            probe_request_id: ProbeKind::CursorColor.request_id(request.observation_id()),
            reuse: ProbeReuse::RefreshRequired,
            sample: Some(CursorColorSample::new(0x00AB_CDEF)),
        })
    }

    fn compatible_probe_report(request: &ObservationRequest) -> CoreEvent {
        CoreEvent::ProbeReported(ProbeReportedEvent::CursorColorReady {
            observation_id: request.observation_id(),
            probe_request_id: ProbeKind::CursorColor.request_id(request.observation_id()),
            reuse: ProbeReuse::Compatible,
            sample: Some(CursorColorSample::new(0x00AB_CDEF)),
        })
    }

    fn background_probe_report(
        request: &ObservationRequest,
        viewport: ViewportSnapshot,
    ) -> CoreEvent {
        CoreEvent::ProbeReported(ProbeReportedEvent::BackgroundReady {
            observation_id: request.observation_id(),
            probe_request_id: ProbeKind::Background.request_id(request.observation_id()),
            reuse: ProbeReuse::Exact,
            batch: BackgroundProbeBatch::empty(viewport),
        })
    }

    fn background_chunk_probe_report(
        request: &ObservationRequest,
        chunk: &BackgroundProbeChunk,
        _viewport: ViewportSnapshot,
    ) -> CoreEvent {
        let allowed_mask = vec![false; chunk.len()];
        CoreEvent::ProbeReported(ProbeReportedEvent::BackgroundChunkReady {
            observation_id: request.observation_id(),
            probe_request_id: ProbeKind::Background.request_id(request.observation_id()),
            chunk: chunk.clone(),
            allowed_mask: BackgroundProbeChunkMask::from_allowed_mask(&allowed_mask),
        })
    }

    struct CoreDispatchTestContext {
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl CoreDispatchTestContext {
        fn new() -> Self {
            let guard = core_dispatch_test_guard();
            replace_core_state(CoreState::default());
            reset_scheduled_effect_queue();
            Self { _guard: guard }
        }

        fn set_core_state(&self, state: CoreState) {
            replace_core_state(state);
        }

        fn dispatch_external_cursor_ingress_to_queue(
            &self,
            observed_at: u64,
        ) -> ObservationRequest {
            dispatch_core_event(external_cursor_demand(observed_at), &mut |effects| {
                // CONTEXT: `stage_batch` reports whether this enqueue operation also needs to arm
                // the drain edge; it does not signal whether the batch was accepted.
                let should_schedule =
                    with_scheduled_effect_queue(|queue| queue.stage_batch(effects).should_schedule);
                assert!(
                    should_schedule,
                    "ingress dispatch should arm exactly one scheduled work item"
                );
            })
            .expect("ingress dispatch should commit reducer state");

            current_core_state()
                .active_observation_request()
                .cloned()
                .expect("ingress dispatch should leave an active observation request")
        }

        fn observing_state_after_base_collection(&self) -> (ObservationRequest, CoreState) {
            self.set_core_state(ready_state_with_cursor_color_probe());
            let observing =
                reduce_core_event(&current_core_state(), external_cursor_demand(25)).next;
            let request = observing
                .active_observation_request()
                .cloned()
                .expect("active observation request");
            let based = reduce_core_event(
                &observing,
                observation_base_collected(
                    &request,
                    observation_basis(&request, Some(cursor(7, 8)), 26),
                ),
            );
            self.set_core_state(based.next.clone());
            (request, based.next)
        }
    }

    impl Drop for CoreDispatchTestContext {
        fn drop(&mut self) {
            replace_core_state(CoreState::default());
            reset_scheduled_effect_queue();
        }
    }

    fn current_core_state() -> CoreState {
        core_state().expect("test core state access should not re-enter")
    }

    fn replace_core_state(state: CoreState) {
        set_core_state(state).expect("test core state write should not re-enter")
    }

    fn external_cursor_demand(observed_at: u64) -> CoreEvent {
        CoreEvent::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(observed_at),
            requested_target: None,
            buffer_perf_class: BufferPerfClass::Full,
            ingress_cursor_presentation: None,
        })
    }

    fn observation_base_collected(
        request: &ObservationRequest,
        basis: ObservationBasis,
    ) -> CoreEvent {
        CoreEvent::ObservationBaseCollected(ObservationBaseCollectedEvent {
            request: request.clone(),
            basis,
            motion: ObservationMotion::default(),
        })
    }

    fn ready_state_with_cursor_color_probe() -> CoreState {
        let mut runtime = ready_state().runtime().clone();
        runtime.config.cursor_color = Some("none".to_string());
        ready_state().with_runtime(runtime)
    }

    fn ready_state_with_cursor_and_background_probes() -> CoreState {
        let mut runtime = ready_state().runtime().clone();
        runtime.config.cursor_color = Some("none".to_string());
        runtime.config.particles_enabled = true;
        runtime.config.particles_over_text = false;
        ready_state().with_runtime(runtime)
    }

    fn sparse_probe_cells(viewport: ViewportSnapshot, count: usize) -> Vec<ScreenCell> {
        let width = i64::from(viewport.max_col.value());
        (0..count)
            .map(|index| {
                let index = i64::try_from(index).expect("probe cell index");
                let row = index / width + 1;
                let col = index % width + 1;
                ScreenCell::new(row, col).expect("probe cell")
            })
            .collect()
    }

    fn install_background_probe_plan(request: &ObservationRequest, basis: &ObservationBasis) {
        let observation =
            ObservationSnapshot::new(request.clone(), basis.clone(), ObservationMotion::default())
                .with_background_probe_plan(BackgroundProbePlan::from_cells(sparse_probe_cells(
                    basis.viewport(),
                    2050,
                )));
        let next = current_core_state()
            .with_last_cursor(Some(cursor(7, 8)))
            .with_active_observation(Some(observation))
            .expect("active observation state");
        replace_core_state(next);
    }

    fn render_cleanup_for_thermal(thermal: RenderThermalState) -> RenderCleanupState {
        let scheduled = RenderCleanupState::scheduled(Millis::new(40), 25, 90, 12);
        match thermal {
            RenderThermalState::Hot => scheduled,
            RenderThermalState::Cooling => scheduled.enter_cooling(Millis::new(65)),
            RenderThermalState::Cold => RenderCleanupState::cold(),
        }
    }

    fn ready_state_with_cleanup_thermal(thermal: RenderThermalState) -> CoreState {
        ready_state().with_render_cleanup(render_cleanup_for_thermal(thermal))
    }

    fn queue_stage_batch(effects: Vec<Effect>) -> bool {
        with_scheduled_effect_queue(|queue| queue.stage_batch(effects).should_schedule)
    }

    fn non_coalescible_effect() -> Effect {
        Effect::ApplyIngressCursorPresentation(IngressCursorPresentationEffect::HideCursor)
    }

    fn schedule_timer_effect(timer_id: TimerId, generation: u64) -> Effect {
        Effect::ScheduleTimer(ScheduleTimerEffect {
            token: TimerToken::new(timer_id, TimerGeneration::new(generation)),
            delay: DelayBudgetMs::try_new(1).expect("positive timer delay"),
            requested_at: Millis::new(generation),
        })
    }

    fn cleanup_effect(max_kept_windows: usize) -> Effect {
        Effect::ApplyRenderCleanup(crate::core::effect::ApplyRenderCleanupEffect {
            execution: crate::core::effect::RenderCleanupExecution::SoftClear { max_kept_windows },
        })
    }

    fn queued_work_count() -> usize {
        with_scheduled_effect_queue(|queue| queue.pending_work_units)
    }

    fn queued_front_work_item() -> Option<ScheduledWorkUnit> {
        with_scheduled_effect_queue(|queue| match queue.items.front()? {
            ScheduledWorkItem::EffectBatch(effects) => {
                Some(ScheduledWorkUnit::EffectBatch(effects.clone()))
            }
            ScheduledWorkItem::CoreEvent(event) => {
                Some(ScheduledWorkUnit::CoreEvent(event.clone()))
            }
            ScheduledWorkItem::EffectOnlyAgenda(agenda) => agenda
                .steps
                .front()
                .cloned()
                .map(ScheduledWorkUnit::EffectOnlyStep),
        })
    }

    fn queue_is_marked_scheduled() -> bool {
        with_scheduled_effect_queue(|queue| queue.drain_scheduled)
    }

    fn drain_next_edge(executor: &mut RecordingExecutor) -> bool {
        drain_scheduled_work_with_executor(executor)
            .expect("scheduled drain should execute one queued edge")
    }

    fn contains_observation_base_request(effects: &[Effect]) -> bool {
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::RequestObservationBase(_)))
    }

    fn contains_probe_request(effects: &[Effect]) -> bool {
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::RequestProbe(_)))
    }

    fn contains_render_plan_request(effects: &[Effect]) -> bool {
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::RequestRenderPlan(_)))
    }

    fn is_apply_proposal(effect: &Effect) -> bool {
        matches!(effect, Effect::ApplyProposal(_))
    }

    fn only_cursor_color_probe_request(effects: &[Effect]) -> bool {
        matches!(
            effects,
            [Effect::RequestProbe(payload)] if payload.kind == ProbeKind::CursorColor
        )
    }

    fn only_background_probe_request_for_chunk(
        effects: &[Effect],
        expected_chunk: &BackgroundProbeChunk,
    ) -> bool {
        matches!(
            effects,
            [Effect::RequestProbe(payload)]
                if payload.kind == ProbeKind::Background
                    && payload.background_chunk.as_ref() == Some(expected_chunk)
        )
    }

    mod dispatch_core_event {
        use super::*;

        #[test]
        fn stages_observation_request_work_for_external_cursor_ingress() {
            let scope = CoreDispatchTestContext::new();
            scope.set_core_state(ready_state());
            let mut staged_batches = Vec::new();

            dispatch_core_event(external_cursor_demand(21), &mut |effects| {
                staged_batches.push(effects)
            })
            .expect("external ingress dispatch should succeed");

            assert_eq!(staged_batches.len(), 1);
            assert!(
                contains_observation_base_request(&staged_batches[0]),
                "expected queued observation request effect"
            );
        }

        #[test]
        fn commits_observing_state_before_shell_work_runs() {
            let scope = CoreDispatchTestContext::new();
            scope.set_core_state(ready_state());

            dispatch_core_event(external_cursor_demand(21), &mut |_| {})
                .expect("external ingress dispatch should succeed");

            let staged_state = current_core_state();
            assert_eq!(staged_state.lifecycle(), Lifecycle::Observing);
            assert!(
                staged_state.active_observation_request().is_some(),
                "dispatch should commit reducer state before shell work runs"
            );
            assert!(
                staged_state.observation().is_none(),
                "observation collection must stay deferred until the scheduled shell edge"
            );
        }
    }

    mod scheduled_effect_drain {
        use super::*;

        fn stage_two_effect_batches() -> CoreDispatchTestContext {
            let scope = CoreDispatchTestContext::new();
            assert!(queue_stage_batch(vec![Effect::RedrawCmdline]));
            assert!(!queue_stage_batch(vec![schedule_timer_effect(
                TimerId::Cleanup,
                1,
            )]));
            scope
        }

        #[test]
        fn first_edge_executes_a_bounded_snapshot_of_existing_batches() {
            let _scope = stage_two_effect_batches();
            let mut executor = RecordingExecutor::default();

            let has_more_items = drain_next_edge(&mut executor);

            assert!(
                !has_more_items,
                "bounded drain should finish the queued snapshot when it fits in budget"
            );
            assert_eq!(
                executor.executed_effects,
                vec![
                    Effect::RedrawCmdline,
                    schedule_timer_effect(TimerId::Cleanup, 1)
                ]
            );
        }

        #[test]
        fn first_edge_clears_the_queue_and_scheduled_flag_when_snapshot_finishes() {
            let _scope = stage_two_effect_batches();
            let mut executor = RecordingExecutor::default();

            let _ = drain_next_edge(&mut executor);

            assert_eq!(queued_work_count(), 0, "queued snapshot should fully drain");
            assert!(
                !queue_is_marked_scheduled(),
                "queue should clear its scheduled flag once the staged snapshot is drained"
            );
        }

        #[test]
        fn first_edge_leaves_new_follow_up_work_for_a_later_edge() {
            let _scope = stage_two_effect_batches();
            replace_core_state(ready_state());
            let mut executor = RecordingExecutor::default();
            executor
                .planned_follow_ups
                .push_back(vec![external_cursor_demand(21)]);
            executor.planned_follow_ups.push_back(Vec::new());

            let has_more_items = drain_next_edge(&mut executor);

            assert!(
                has_more_items,
                "follow-up work staged during the drain should remain deferred"
            );
            assert_eq!(
                executor.executed_effects,
                vec![
                    Effect::RedrawCmdline,
                    schedule_timer_effect(TimerId::Cleanup, 1)
                ]
            );
            assert!(
                matches!(
                    queued_front_work_item(),
                    Some(ScheduledWorkUnit::EffectBatch(ref effects))
                        if contains_observation_base_request(effects)
                ),
                "newly staged follow-up work should remain queued for the next scheduled edge"
            );
            assert!(
                queue_is_marked_scheduled(),
                "queue must stay armed when new follow-up work remains"
            );
        }

        #[test]
        fn hot_edge_continues_through_cleanup_follow_up_waves() {
            let _scope = CoreDispatchTestContext::new();
            replace_core_state(ready_state_with_cleanup_thermal(RenderThermalState::Hot));
            assert!(queue_stage_batch(vec![Effect::ApplyRenderCleanup(
                crate::core::effect::ApplyRenderCleanupEffect {
                    execution: crate::core::effect::RenderCleanupExecution::SoftClear {
                        max_kept_windows: 12,
                    },
                },
            )]));
            let mut executor = RecordingExecutor::default();
            executor
                .planned_follow_ups
                .push_back(vec![CoreEvent::RenderCleanupApplied(
                    crate::core::event::RenderCleanupAppliedEvent {
                        observed_at: Millis::new(65),
                        action: crate::core::event::RenderCleanupAppliedAction::SoftCleared,
                    },
                )]);

            let has_more_items = drain_next_edge(&mut executor);

            assert!(
                !has_more_items,
                "hot drain should finish cleanup-only follow-up waves without rearming"
            );
            assert_eq!(
                executor.executed_effects.len(),
                2,
                "follow-up cleanup work should run in the same scheduled callback"
            );
            assert!(
                matches!(
                    executor.executed_effects.get(1),
                    Some(Effect::ApplyRenderCleanup(_))
                ),
                "follow-up compaction should stay ordered after the original cleanup effect"
            );
            assert_eq!(
                queued_work_count(),
                0,
                "continued hot drain should leave no queue tail"
            );
            assert!(
                !queue_is_marked_scheduled(),
                "queue should disarm when the hot follow-up chain finishes"
            );
        }

        #[test]
        fn drain_failure_resets_the_scheduled_queue_state() {
            let _scope = stage_two_effect_batches();
            let mut executor = FailingExecutor;

            let err = drain_scheduled_work_with_executor(&mut executor)
                .expect_err("planned executor failure should surface from the drain");
            let _ = err;
            reset_scheduled_queue_after_failure();

            assert_eq!(
                queued_work_count(),
                0,
                "failure reset should clear queued work"
            );
            assert!(
                !queue_is_marked_scheduled(),
                "failure reset should disarm the scheduled drain flag"
            );
        }

        #[test]
        fn hot_budget_clears_small_queues_and_caps_large_backlogs() {
            assert_eq!(
                scheduled_drain_budget_for_thermal(RenderThermalState::Hot, 0),
                0
            );
            assert_eq!(
                scheduled_drain_budget_for_thermal(RenderThermalState::Hot, 3),
                3
            );
            assert_eq!(
                scheduled_drain_budget_for_thermal(RenderThermalState::Hot, 16),
                16
            );
            assert_eq!(
                scheduled_drain_budget_for_thermal(RenderThermalState::Hot, 24),
                16
            );
            assert_eq!(
                scheduled_drain_budget_for_thermal(RenderThermalState::Hot, 40),
                20
            );
            assert_eq!(
                scheduled_drain_budget_for_thermal(RenderThermalState::Hot, 117),
                32
            );
        }

        #[test]
        fn hot_effect_only_budget_expands_to_a_single_snapshot_up_to_the_chain_cap() {
            assert_eq!(scheduled_drain_budget_for_hot_effect_only_snapshot(0), 0);
            assert_eq!(scheduled_drain_budget_for_hot_effect_only_snapshot(3), 3);
            assert_eq!(scheduled_drain_budget_for_hot_effect_only_snapshot(40), 40);
            assert_eq!(scheduled_drain_budget_for_hot_effect_only_snapshot(117), 96);
        }

        #[test]
        fn cooling_budget_expands_to_the_full_queued_snapshot() {
            assert_eq!(
                scheduled_drain_budget_for_thermal(RenderThermalState::Cooling, 0),
                0
            );
            assert_eq!(
                scheduled_drain_budget_for_thermal(RenderThermalState::Cooling, 3),
                3
            );
            assert_eq!(
                scheduled_drain_budget_for_thermal(RenderThermalState::Cooling, 40),
                40
            );
        }

        #[test]
        fn hot_effect_only_backlog_drains_the_full_snapshot_without_rearming() {
            let _scope = CoreDispatchTestContext::new();
            replace_core_state(ready_state_with_cleanup_thermal(RenderThermalState::Hot));
            for index in 0..40 {
                let should_schedule = queue_stage_batch(vec![cleanup_effect(index + 1)]);
                if index == 0 {
                    assert!(
                        should_schedule,
                        "first effect-only item should arm the drain"
                    );
                } else {
                    assert!(
                        !should_schedule,
                        "later effect-only items should reuse the armed drain"
                    );
                }
            }
            let mut executor = RecordingExecutor::default();

            let has_more_items = drain_next_edge(&mut executor);

            assert!(
                !has_more_items,
                "hot effect-only snapshot should converge in one scheduled edge"
            );
            assert_eq!(
                executor.executed_effects.len(),
                40,
                "hot effect-only queues should drain the entire queued snapshot"
            );
            assert_eq!(queued_work_count(), 0, "drain should leave no queued tail");
            assert!(
                !queue_is_marked_scheduled(),
                "queue should disarm once the effect-only snapshot converges"
            );
        }

        #[test]
        fn hot_effect_only_snapshot_still_defers_mid_pass_follow_up_work() {
            let _scope = CoreDispatchTestContext::new();
            replace_core_state(ready_state_with_cleanup_thermal(RenderThermalState::Hot));
            assert!(queue_stage_batch(vec![cleanup_effect(12)]));
            assert!(!queue_stage_batch(vec![cleanup_effect(13)]));
            let mut executor = RecordingExecutor::default();
            executor
                .planned_follow_ups
                .push_back(vec![external_cursor_demand(21)]);
            executor.planned_follow_ups.push_back(Vec::new());

            let has_more_items = drain_next_edge(&mut executor);

            assert!(
                has_more_items,
                "new reducer follow-up work should remain deferred after the hot effect-only snapshot"
            );
            assert_eq!(
                executor.executed_effects,
                vec![cleanup_effect(12), cleanup_effect(13)],
                "the original effect-only snapshot should still drain in FIFO order"
            );
            assert!(
                matches!(
                    queued_front_work_item(),
                    Some(ScheduledWorkUnit::EffectBatch(ref effects))
                        if contains_observation_base_request(effects)
                ),
                "mid-pass reducer follow-up work should remain queued for the next edge"
            );
            assert!(
                queue_is_marked_scheduled(),
                "queue should stay armed when deferred follow-up work remains"
            );
        }

        #[test]
        fn hot_mixed_snapshot_continues_when_remaining_tail_is_effect_only() {
            let _scope = CoreDispatchTestContext::new();
            replace_core_state(ready_state_with_cleanup_thermal(RenderThermalState::Hot));
            for index in 0..16 {
                let should_schedule = queue_stage_batch(vec![non_coalescible_effect()]);
                if index == 0 {
                    assert!(should_schedule, "first staged item should arm the drain");
                } else {
                    assert!(
                        !should_schedule,
                        "later staged items should reuse the armed drain edge"
                    );
                }
            }
            assert!(!queue_stage_batch(vec![cleanup_effect(12)]));
            assert!(!queue_stage_batch(vec![cleanup_effect(13)]));
            let mut executor = RecordingExecutor::default();

            let has_more_items = drain_next_edge(&mut executor);

            assert!(
                !has_more_items,
                "once the mixed prefix drains, the effect-only tail should finish in the same hot callback"
            );
            assert_eq!(
                executor.executed_effects.len(),
                18,
                "the hot drain should execute both the bounded mixed prefix and the effect-only tail"
            );
            assert!(
                executor.executed_effects[..16]
                    .iter()
                    .all(|effect| *effect == non_coalescible_effect()),
                "the bounded mixed prefix should preserve FIFO order before the effect-only tail"
            );
            assert_eq!(executor.executed_effects.get(16), Some(&cleanup_effect(12)));
            assert_eq!(executor.executed_effects.get(17), Some(&cleanup_effect(13)));
            assert_eq!(
                queued_work_count(),
                0,
                "continued hot drain should leave no queue tail"
            );
            assert!(
                !queue_is_marked_scheduled(),
                "queue should disarm once the mixed snapshot collapses to an effect-only tail"
            );
        }

        #[test]
        fn hot_first_edge_reschedules_when_backlog_exceeds_the_bounded_fractional_budget() {
            let _scope = CoreDispatchTestContext::new();
            replace_core_state(ready_state_with_cleanup_thermal(RenderThermalState::Hot));
            for _ in 0..40 {
                let should_schedule = queue_stage_batch(vec![non_coalescible_effect()]);
                if queued_work_count() == 1 {
                    assert!(should_schedule, "first staged item should arm the drain");
                } else {
                    assert!(
                        !should_schedule,
                        "subsequent staged items should reuse the armed drain edge"
                    );
                }
            }
            let mut executor = RecordingExecutor::default();

            let has_more_items = drain_next_edge(&mut executor);

            assert!(
                has_more_items,
                "large backlog should keep work queued for a later edge"
            );
            assert_eq!(
                executor.executed_effects.len(),
                20,
                "40 queued work units should drain half the backlog on the first edge"
            );
            assert_eq!(
                queued_work_count(),
                20,
                "remaining backlog should stay queued after the first bounded drain"
            );
            assert!(
                queue_is_marked_scheduled(),
                "queue should remain armed while bounded backlog remains"
            );
        }

        #[test]
        fn adjacent_redraw_batches_coalesce_into_one_work_unit() {
            let _scope = CoreDispatchTestContext::new();

            assert!(queue_stage_batch(vec![Effect::RedrawCmdline]));
            for _ in 0..39 {
                assert!(
                    !queue_stage_batch(vec![Effect::RedrawCmdline]),
                    "adjacent redraw work should reuse the same agenda and drain edge"
                );
            }

            assert_eq!(queued_work_count(), 1, "redraw waves should coalesce");

            let mut executor = RecordingExecutor::default();
            let has_more_items = drain_next_edge(&mut executor);

            assert!(!has_more_items);
            assert_eq!(executor.executed_effects, vec![Effect::RedrawCmdline]);
            assert_eq!(queued_work_count(), 0);
        }

        #[test]
        fn adjacent_metric_batches_share_one_work_unit() {
            let _scope = CoreDispatchTestContext::new();

            assert!(queue_stage_batch(vec![Effect::RecordEventLoopMetric(
                EventLoopMetricEffect::StaleToken,
            )]));
            assert!(!queue_stage_batch(vec![Effect::RecordEventLoopMetric(
                EventLoopMetricEffect::DelayedIngressPendingUpdated,
            )]));

            assert_eq!(
                queued_work_count(),
                1,
                "adjacent metric work should aggregate"
            );
        }

        #[test]
        fn adjacent_timer_rearms_for_the_same_kind_keep_only_the_newest_effect() {
            let _scope = CoreDispatchTestContext::new();

            assert!(queue_stage_batch(vec![schedule_timer_effect(
                TimerId::Cleanup,
                1,
            )]));
            assert!(!queue_stage_batch(vec![schedule_timer_effect(
                TimerId::Cleanup,
                2,
            )]));

            assert_eq!(
                queued_work_count(),
                1,
                "same-kind timer rearm should replace in place"
            );

            let mut executor = RecordingExecutor::default();
            let has_more_items = drain_next_edge(&mut executor);

            assert!(!has_more_items);
            assert_eq!(
                executor.executed_effects,
                vec![schedule_timer_effect(TimerId::Cleanup, 2)]
            );
        }

        #[test]
        fn hot_bounded_drain_converges_exactly_for_a_range_of_backlog_depths() {
            for queued_items in 1..=96 {
                let _scope = CoreDispatchTestContext::new();
                replace_core_state(ready_state_with_cleanup_thermal(RenderThermalState::Hot));
                for _ in 0..queued_items {
                    let should_schedule = queue_stage_batch(vec![non_coalescible_effect()]);
                    if queued_work_count() == 1 {
                        assert!(
                            should_schedule,
                            "first staged item should arm the drain for backlog {queued_items}"
                        );
                    } else {
                        assert!(
                            !should_schedule,
                            "later staged items should reuse the armed drain for backlog {queued_items}"
                        );
                    }
                }
                let mut executor = RecordingExecutor::default();

                let has_more_items = drain_next_edge(&mut executor);
                let expected_drained = scheduled_drain_budget_for_depth(queued_items);
                let expected_remaining = queued_items.saturating_sub(expected_drained);

                assert_eq!(
                    executor.executed_effects.len(),
                    expected_drained,
                    "drain edge should process the exact bounded budget for backlog {queued_items}"
                );
                assert_eq!(
                    queued_work_count(),
                    expected_remaining,
                    "queued backlog should shrink by the exact bounded budget for backlog {queued_items}"
                );
                assert_eq!(
                    has_more_items,
                    expected_remaining > 0,
                    "drain outcome should report whether bounded backlog remains for backlog {queued_items}"
                );
                assert_eq!(
                    queue_is_marked_scheduled(),
                    expected_remaining > 0,
                    "drain scheduling flag should track whether queued backlog remains for backlog {queued_items}"
                );
            }
        }

        #[test]
        fn cooling_convergence_leaves_no_queue_tail_after_idle_backlog_drains() {
            let _scope = CoreDispatchTestContext::new();
            replace_core_state(ready_state_with_cleanup_thermal(
                RenderThermalState::Cooling,
            ));
            for _ in 0..40 {
                let should_schedule = queue_stage_batch(vec![non_coalescible_effect()]);
                if queued_work_count() == 1 {
                    assert!(should_schedule, "first staged item should arm the drain");
                } else {
                    assert!(
                        !should_schedule,
                        "subsequent staged items should reuse the armed drain edge"
                    );
                }
            }
            let mut executor = RecordingExecutor::default();

            let has_more_items = drain_next_edge(&mut executor);

            assert!(
                !has_more_items,
                "cooling should converge the staged backlog on the first drain edge"
            );
            assert_eq!(
                executor.executed_effects.len(),
                40,
                "cooling should drain the full queued snapshot instead of a bounded fraction"
            );
            assert_eq!(
                queued_work_count(),
                0,
                "cooling should not leave a bounded tail from the pre-existing backlog"
            );
            assert_eq!(
                scheduled_drain_budget(),
                0,
                "idle convergence should leave no remaining scheduled-drain budget"
            );
            assert!(
                !queue_is_marked_scheduled(),
                "queue should disarm once cooling drains the existing backlog to zero"
            );
        }

        #[test]
        fn cooling_snapshot_drain_still_defers_mid_pass_follow_up_work() {
            let _scope = CoreDispatchTestContext::new();
            replace_core_state(ready_state_with_cleanup_thermal(
                RenderThermalState::Cooling,
            ));
            assert!(queue_stage_batch(vec![Effect::RedrawCmdline]));
            let mut executor = RecordingExecutor::default();
            executor
                .planned_follow_ups
                .push_back(vec![external_cursor_demand(21)]);

            let has_more_items = drain_next_edge(&mut executor);

            assert!(
                has_more_items,
                "follow-up work staged during cooling should remain deferred to the next edge"
            );
            assert_eq!(
                executor.executed_effects,
                vec![Effect::RedrawCmdline],
                "cooling should still execute the original staged snapshot in order"
            );
            assert!(
                matches!(
                    queued_front_work_item(),
                    Some(ScheduledWorkUnit::EffectBatch(ref effects))
                        if contains_observation_base_request(effects)
                ),
                "mid-pass follow-up work should stay queued after the cooling snapshot drains"
            );
            assert!(
                queue_is_marked_scheduled(),
                "queue should remain armed when deferred follow-up work is still pending"
            );
        }
    }

    mod refresh_required_probe_retry {
        use super::*;

        fn setup_refresh_required_retry() -> (
            CoreDispatchTestContext,
            ObservationRequest,
            CoreState,
            CoreEvent,
            RecordingExecutor,
        ) {
            let scope = CoreDispatchTestContext::new();
            let (request, based_state) = scope.observing_state_after_base_collection();
            let refresh_required = refresh_required_probe_report(&request);
            assert!(queue_stage_batch(vec![Effect::RedrawCmdline]));
            let executor = RecordingExecutor {
                planned_follow_ups: VecDeque::from([vec![refresh_required.clone()]]),
                ..RecordingExecutor::default()
            };
            (scope, request, based_state, refresh_required, executor)
        }

        #[test]
        fn probe_edge_requeues_refresh_required_follow_up_as_a_core_event() {
            let (_scope, _request, _based_state, refresh_required, mut executor) =
                setup_refresh_required_retry();

            let has_more_items = drain_next_edge(&mut executor);

            assert!(
                has_more_items,
                "refresh-required probe follow-up should remain queued for a later edge"
            );
            assert_eq!(
                queued_work_count(),
                1,
                "retry event should be queued explicitly"
            );
            assert!(matches!(
                queued_front_work_item(),
                Some(ScheduledWorkUnit::CoreEvent(event)) if *event == refresh_required
            ));
        }

        #[test]
        fn probe_edge_leaves_the_active_state_unchanged_until_the_retry_event_runs() {
            let (_scope, _request, based_state, _refresh_required, mut executor) =
                setup_refresh_required_retry();

            let _ = drain_next_edge(&mut executor);

            assert_eq!(current_core_state(), based_state);
        }

        #[test]
        fn retry_edge_keeps_the_active_request_authoritative() {
            let (_scope, request, _based_state, _refresh_required, mut executor) =
                setup_refresh_required_retry();

            let _ = drain_next_edge(&mut executor);
            let _ = drain_next_edge(&mut executor);

            let retried_state = current_core_state();
            assert_eq!(retried_state.lifecycle(), Lifecycle::Observing);
            assert_eq!(retried_state.active_observation_request(), Some(&request));
        }

        #[test]
        fn retry_edge_clears_the_mixed_world_observation_before_replay() {
            let (_scope, _request, _based_state, _refresh_required, mut executor) =
                setup_refresh_required_retry();

            let _ = drain_next_edge(&mut executor);
            let _ = drain_next_edge(&mut executor);

            assert!(
                current_core_state().observation().is_none(),
                "refresh-required retry should clear retained observation data before replay"
            );
        }

        #[test]
        fn retry_edge_stages_a_new_observation_base_request_for_a_later_edge() {
            let (_scope, _request, _based_state, _refresh_required, mut executor) =
                setup_refresh_required_retry();

            let _ = drain_next_edge(&mut executor);
            let has_more_items = drain_next_edge(&mut executor);

            assert!(
                has_more_items,
                "retry transition should stage a later observation batch"
            );
            assert_eq!(queued_work_count(), 1);
            assert!(matches!(
                queued_front_work_item(),
                Some(ScheduledWorkUnit::EffectBatch(ref effects))
                    if contains_observation_base_request(effects)
            ));
        }
    }

    mod single_cursor_probe_observation {
        use super::*;

        fn setup_cursor_probe_ingress() -> (CoreDispatchTestContext, ObservationRequest) {
            let scope = CoreDispatchTestContext::new();
            scope.set_core_state(ready_state_with_cursor_color_probe());
            let request = scope.dispatch_external_cursor_ingress_to_queue(25);
            (scope, request)
        }

        #[test]
        fn ingress_dispatch_queues_one_observation_base_batch() {
            let (_scope, _request) = setup_cursor_probe_ingress();
            let after_ingress = current_core_state();

            assert_eq!(after_ingress.lifecycle(), Lifecycle::Observing);
            assert!(after_ingress.observation().is_none());
            assert!(after_ingress.pending_proposal().is_none());
            assert_eq!(
                queued_work_count(),
                1,
                "ingress should queue one effect batch"
            );
            assert!(matches!(
                queued_front_work_item(),
                Some(ScheduledWorkUnit::EffectBatch(ref effects))
                    if contains_observation_base_request(effects)
            ));
        }

        #[test]
        fn observation_base_edge_executes_the_same_wave_cursor_probe() {
            let (_scope, request) = setup_cursor_probe_ingress();
            let mut executor = RecordingExecutor::default();
            executor
                .planned_follow_ups
                .push_back(vec![observation_base_collected(
                    &request,
                    observation_basis(&request, Some(cursor(7, 8)), 26),
                )]);
            executor.planned_follow_ups.push_back(Vec::new());
            executor
                .planned_follow_ups
                .push_back(vec![compatible_probe_report(&request)]);

            let has_more_items = drain_next_edge(&mut executor);

            assert!(
                has_more_items,
                "same-wave cursor probe completion should still leave planning/apply work for a later edge"
            );
            assert!(matches!(
                executor.executed_effects.as_slice(),
                [
                    Effect::RequestObservationBase(_),
                    Effect::ScheduleTimer(_),
                    Effect::RequestProbe(RequestProbeEffect {
                        kind: ProbeKind::CursorColor,
                        ..
                    })
                ]
            ));
        }

        #[test]
        fn observation_base_edge_updates_the_retained_observation_from_same_wave_probe() {
            let (_scope, request) = setup_cursor_probe_ingress();
            let mut executor = RecordingExecutor::default();
            executor
                .planned_follow_ups
                .push_back(vec![observation_base_collected(
                    &request,
                    observation_basis(&request, Some(cursor(7, 8)), 26),
                )]);
            executor.planned_follow_ups.push_back(Vec::new());
            executor
                .planned_follow_ups
                .push_back(vec![compatible_probe_report(&request)]);

            let _ = drain_next_edge(&mut executor);

            assert_eq!(
                current_core_state()
                    .observation()
                    .and_then(crate::core::state::ObservationSnapshot::cursor_color),
                Some(0x00AB_CDEF)
            );
        }

        fn setup_after_same_wave_cursor_probe() -> (
            CoreDispatchTestContext,
            ObservationRequest,
            RecordingExecutor,
        ) {
            let (scope, request) = setup_cursor_probe_ingress();
            let mut executor = RecordingExecutor::default();
            executor
                .planned_follow_ups
                .push_back(vec![observation_base_collected(
                    &request,
                    observation_basis(&request, Some(cursor(7, 8)), 26),
                )]);
            executor.planned_follow_ups.push_back(Vec::new());
            executor
                .planned_follow_ups
                .push_back(vec![compatible_probe_report(&request)]);
            let _ = drain_next_edge(&mut executor);
            (scope, request, executor)
        }

        #[test]
        fn same_wave_cursor_probe_keeps_apply_work_deferred() {
            let (_scope, _request, executor) = setup_after_same_wave_cursor_probe();

            assert!(
                !executor.executed_effects.iter().any(is_apply_proposal),
                "apply work must remain deferred after the same-wave probe finishes because planning still runs first"
            );
        }

        #[test]
        fn same_wave_cursor_probe_leaves_only_non_probe_follow_up_work_queued() {
            let (_scope, _request, _executor) = setup_after_same_wave_cursor_probe();
            let has_more_items = queued_work_count() > 0;
            let queued_follow_up = queued_front_work_item();

            assert!(
                if has_more_items {
                    matches!(
                        queued_follow_up,
                        Some(ScheduledWorkUnit::EffectBatch(ref effects))
                            if !contains_probe_request(effects)
                                && (contains_render_plan_request(effects)
                                    || effects.iter().any(is_apply_proposal))
                    )
                } else {
                    queued_follow_up.is_none()
                },
                "same-wave probe completion should either queue planning/apply work or finish without extra shell work"
            );
        }
    }

    mod deferred_multi_probe_observation {
        use super::*;

        fn setup_multi_probe_ingress() -> (
            CoreDispatchTestContext,
            ObservationRequest,
            ObservationBasis,
        ) {
            let scope = CoreDispatchTestContext::new();
            scope.set_core_state(ready_state_with_cursor_and_background_probes());
            let request = scope.dispatch_external_cursor_ingress_to_queue(25);
            let basis = observation_basis(&request, Some(cursor(7, 8)), 26);
            (scope, request, basis)
        }

        fn setup_after_observation_base_edge() -> (
            CoreDispatchTestContext,
            ObservationRequest,
            ObservationBasis,
            RecordingExecutor,
        ) {
            let (scope, request, basis) = setup_multi_probe_ingress();
            let mut executor = RecordingExecutor::default();
            executor
                .planned_follow_ups
                .push_back(vec![observation_base_collected(&request, basis.clone())]);
            let _ = drain_next_edge(&mut executor);
            install_background_probe_plan(&request, &basis);
            (scope, request, basis, executor)
        }

        fn setup_after_cursor_color_probe_edge() -> (
            CoreDispatchTestContext,
            ObservationRequest,
            ObservationBasis,
            BackgroundProbeChunk,
            RecordingExecutor,
        ) {
            let (scope, request, basis, mut executor) = setup_after_observation_base_edge();
            executor
                .planned_follow_ups
                .push_back(vec![compatible_probe_report(&request)]);
            let _ = drain_next_edge(&mut executor);
            let first_background_chunk = current_core_state()
                .observation()
                .and_then(|observation| observation.background_progress())
                .and_then(crate::core::state::BackgroundProbeProgress::next_chunk)
                .expect("first background chunk");
            (scope, request, basis, first_background_chunk, executor)
        }

        #[test]
        fn observation_base_edge_queues_only_the_cursor_color_probe() {
            let (_scope, _request, _basis, executor) = setup_after_observation_base_edge();

            assert!(matches!(
                executor.executed_effects.as_slice(),
                [Effect::RequestObservationBase(_), Effect::ScheduleTimer(_)]
            ));
            assert_eq!(
                queued_work_count(),
                1,
                "only one probe batch should be queued"
            );
            assert!(matches!(
                queued_front_work_item(),
                Some(ScheduledWorkUnit::EffectBatch(ref effects))
                    if only_cursor_color_probe_request(effects)
            ));
        }

        #[test]
        fn cursor_color_probe_edge_queues_the_first_background_chunk() {
            let (_scope, _request, _basis, first_background_chunk, executor) =
                setup_after_cursor_color_probe_edge();

            assert!(
                executor.executed_effects.iter().any(|effect| matches!(
                    effect,
                    Effect::RequestProbe(payload) if payload.kind == ProbeKind::CursorColor
                )),
                "cursor-color edge should execute the cursor-color probe request",
            );
            assert_eq!(queued_work_count(), 1);
            assert!(matches!(
                queued_front_work_item(),
                Some(ScheduledWorkUnit::EffectBatch(ref effects))
                    if only_background_probe_request_for_chunk(effects, &first_background_chunk)
            ));
        }

        #[test]
        fn background_chunk_edge_queues_the_next_background_chunk() {
            let (_scope, request, basis, first_background_chunk, mut executor) =
                setup_after_cursor_color_probe_edge();
            executor
                .planned_follow_ups
                .push_back(vec![background_chunk_probe_report(
                    &request,
                    &first_background_chunk,
                    basis.viewport(),
                )]);

            let has_more_items = drain_next_edge(&mut executor);
            let second_background_chunk = current_core_state()
                .observation()
                .and_then(|observation| observation.background_progress())
                .and_then(crate::core::state::BackgroundProbeProgress::next_chunk)
                .expect("second background chunk");

            assert!(
                has_more_items,
                "background chunk completion should queue the next chunk"
            );
            assert!(
                executor.executed_effects.iter().any(|effect| matches!(
                    effect,
                    Effect::RequestProbe(payload)
                        if payload.kind == ProbeKind::Background
                            && payload.background_chunk.as_ref() == Some(&first_background_chunk)
                )),
                "background-chunk edge should execute the current background probe request",
            );
            assert_eq!(queued_work_count(), 1);
            assert!(matches!(
                queued_front_work_item(),
                Some(ScheduledWorkUnit::EffectBatch(ref effects))
                    if only_background_probe_request_for_chunk(effects, &second_background_chunk)
            ));
        }

        #[test]
        fn final_background_edge_transitions_the_runtime_to_planning() {
            let (_scope, request, basis, first_background_chunk, mut executor) =
                setup_after_cursor_color_probe_edge();
            executor
                .planned_follow_ups
                .push_back(vec![background_chunk_probe_report(
                    &request,
                    &first_background_chunk,
                    basis.viewport(),
                )]);
            let _ = drain_next_edge(&mut executor);
            executor
                .planned_follow_ups
                .push_back(vec![background_probe_report(&request, basis.viewport())]);

            let _ = drain_next_edge(&mut executor);
            let after_completion = current_core_state();

            assert_eq!(after_completion.lifecycle(), Lifecycle::Planning);
            assert!(after_completion.pending_proposal().is_none());
            assert!(after_completion.pending_plan_proposal_id().is_some());
        }

        #[test]
        fn final_background_edge_queues_render_plan_work_for_a_later_edge() {
            let (_scope, request, basis, first_background_chunk, mut executor) =
                setup_after_cursor_color_probe_edge();
            executor
                .planned_follow_ups
                .push_back(vec![background_chunk_probe_report(
                    &request,
                    &first_background_chunk,
                    basis.viewport(),
                )]);
            let _ = drain_next_edge(&mut executor);
            executor
                .planned_follow_ups
                .push_back(vec![background_probe_report(&request, basis.viewport())]);

            let has_more_items = drain_next_edge(&mut executor);

            assert!(
                has_more_items,
                "planning work should remain deferred to a later edge"
            );
            assert_eq!(queued_work_count(), 1, "planning work should remain queued");
            assert!(matches!(
                queued_front_work_item(),
                Some(ScheduledWorkUnit::EffectBatch(ref effects))
                    if contains_render_plan_request(effects)
            ));
        }
    }
}
