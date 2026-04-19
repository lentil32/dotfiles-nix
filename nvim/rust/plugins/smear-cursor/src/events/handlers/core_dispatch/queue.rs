use crate::core::effect::ApplyRenderCleanupEffect;
use crate::core::effect::Effect;
use crate::core::effect::EventLoopMetricEffect;
use crate::core::effect::ScheduleTimerEffect;
use crate::core::effect::TimerKind;
use crate::core::event::Event as CoreEvent;
use crate::core::state::ProbeKind;
use crate::core::types::Millis;
use std::cell::RefCell;
use std::collections::VecDeque;

pub(super) const MIN_SCHEDULED_WORK_ITEMS_PER_EDGE: usize = 16;
pub(super) const MAX_SCHEDULED_WORK_ITEMS_PER_EDGE: usize = 32;
pub(super) const MAX_CHAINED_SCHEDULED_WORK_ITEMS_PER_EDGE: usize = 96;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum ScheduledEffectDrainEntry {
    NextItem,
}

impl ScheduledEffectDrainEntry {
    pub(super) const fn context(self) -> &'static str {
        match self {
            Self::NextItem => "core effect drain",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum ScheduledWorkItem {
    EffectBatch(Vec<Effect>),
    CoreEvent(Box<CoreEvent>),
    EffectOnlyAgenda(EffectOnlyAgenda),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum ScheduledWorkUnit {
    EffectBatch(Vec<Effect>),
    CoreEvent(Box<CoreEvent>),
    EffectOnlyStep(EffectOnlyStep),
}

// Shell-queue-only copies of continuable effects. These payloads may survive until
// the next scheduled drain step, but they must not become reducer-owned state.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct EffectOnlyAgenda {
    pub(super) steps: VecDeque<EffectOnlyStep>,
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
pub(super) enum EffectOnlyStep {
    ApplyRenderCleanup(ApplyRenderCleanupEffect),
    ScheduleTimer(ScheduleTimerEffect),
    RecordMetrics(PendingMetricEffects),
    RedrawCmdline,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct PendingMetricEffects {
    pub(super) ingress_coalesced: usize,
    pub(super) delayed_ingress_pending_updated: usize,
    pub(super) stale_token: usize,
    pub(super) cleanup_converged_to_cold: Vec<(Millis, Millis)>,
    pub(super) cursor_color_probe_refresh_retried: usize,
    pub(super) background_probe_refresh_retried: usize,
    pub(super) cursor_color_probe_refresh_budget_exhausted: usize,
    pub(super) background_probe_refresh_budget_exhausted: usize,
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
pub(super) struct ScheduledEffectQueueState {
    pub(super) items: VecDeque<ScheduledWorkItem>,
    pub(super) pending_work_units: usize,
    pub(super) drain_scheduled: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct ScheduledStageResult {
    pub(super) should_schedule: bool,
    pub(super) depth: usize,
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

    pub(super) fn stage_batch(&mut self, effects: Vec<Effect>) -> ScheduledStageResult {
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

    pub(super) fn stage_core_event(&mut self, event: CoreEvent) -> ScheduledStageResult {
        self.stage_irreducible_item(ScheduledWorkItem::CoreEvent(Box::new(event)))
    }

    pub(super) fn pop_work_unit(&mut self) -> Option<ScheduledWorkUnit> {
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
                let (step, agenda_is_empty) = match self.items.front_mut() {
                    Some(ScheduledWorkItem::EffectOnlyAgenda(agenda)) => {
                        let Some(step) = agenda.pop_step() else {
                            unreachable!("non-empty effect-only agenda should yield a step");
                        };
                        (step, agenda.is_empty())
                    }
                    _ => unreachable!("front queue item should stay stable while borrowed"),
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

    pub(super) fn reset(&mut self) {
        self.items.clear();
        self.pending_work_units = 0;
        self.drain_scheduled = false;
    }
}

thread_local! {
    static SCHEDULED_EFFECT_QUEUE: RefCell<ScheduledEffectQueueState> =
        RefCell::new(ScheduledEffectQueueState::default());
}

pub(super) fn with_scheduled_effect_queue<R>(
    mutator: impl FnOnce(&mut ScheduledEffectQueueState) -> R,
) -> R {
    SCHEDULED_EFFECT_QUEUE.with(|queue| {
        // Keep queue borrows scoped to staging/pop bookkeeping only. Reducer execution and effect
        // dispatch always happen after this borrow is released, so re-entering here would signal a
        // structural bug we should fix directly instead of silently dropping queued work.
        let mut queue = queue.borrow_mut();
        mutator(&mut queue)
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
