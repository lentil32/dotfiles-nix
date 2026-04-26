use crate::core::effect::Effect;
use crate::core::effect::EventLoopMetricEffect;
use crate::core::effect::OrderedEffect;
use crate::core::effect::ShellOnlyEffect;
use crate::core::event::Event as CoreEvent;
use crate::core::state::ProbeKind;
use crate::core::types::Millis;
use std::collections::VecDeque;

pub(in crate::events) const MIN_SCHEDULED_WORK_ITEMS_PER_EDGE: usize = 16;
pub(in crate::events) const MAX_SCHEDULED_WORK_ITEMS_PER_EDGE: usize = 32;
pub(in crate::events) const MAX_CHAINED_SCHEDULED_WORK_ITEMS_PER_EDGE: usize = 96;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(in crate::events) enum ScheduledEffectDrainEntry {
    NextItem,
}

impl ScheduledEffectDrainEntry {
    pub(in crate::events) const fn context(self) -> &'static str {
        match self {
            Self::NextItem => "core effect drain",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::events) enum ScheduledWorkItem {
    OrderedEffectBatch(Vec<OrderedEffect>),
    CoreEvent(Box<CoreEvent>),
    ShellOnlyAgenda(ShellOnlyAgenda),
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::events) enum ScheduledWorkUnit {
    OrderedEffectBatch(Vec<OrderedEffect>),
    CoreEvent(Box<CoreEvent>),
    ShellOnlyStep(ShellOnlyStep),
}

#[derive(Debug, Clone, PartialEq)]
enum EffectBatchSegment {
    Ordered(Vec<OrderedEffect>),
    ShellOnly(Vec<ShellOnlyEffect>),
}

// Shell-queue-only copies of coalescible effects. These payloads may survive until
// the next scheduled drain step, but they cannot feed semantic information back
// into the reducer.
#[derive(Debug, Clone, Default, PartialEq)]
pub(in crate::events) struct ShellOnlyAgenda {
    pub(in crate::events) steps: VecDeque<ShellOnlyStep>,
}

impl ShellOnlyAgenda {
    fn append_effects(&mut self, effects: Vec<ShellOnlyEffect>) -> usize {
        effects
            .into_iter()
            .map(|effect| usize::from(self.append_effect(effect)))
            .sum()
    }

    fn append_effect(&mut self, effect: ShellOnlyEffect) -> bool {
        match effect {
            ShellOnlyEffect::RecordEventLoopMetric(metric) => {
                if let Some(ShellOnlyStep::RecordMetrics(metrics)) = self.steps.back_mut() {
                    metrics.record(metric);
                    return false;
                }
                let mut metrics = PendingMetricEffects::default();
                metrics.record(metric);
                self.steps.push_back(ShellOnlyStep::RecordMetrics(metrics));
                true
            }
            ShellOnlyEffect::RedrawCmdline => {
                if matches!(self.steps.back(), Some(ShellOnlyStep::RedrawCmdline)) {
                    return false;
                }
                self.steps.push_back(ShellOnlyStep::RedrawCmdline);
                true
            }
        }
    }

    fn pop_step(&mut self) -> Option<ShellOnlyStep> {
        self.steps.pop_front()
    }

    fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::events) enum ShellOnlyStep {
    RecordMetrics(PendingMetricEffects),
    RedrawCmdline,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(in crate::events) struct PendingMetricEffects {
    pub(in crate::events) ingress_coalesced: usize,
    pub(in crate::events) delayed_ingress_pending_updated: usize,
    pub(in crate::events) stale_token: usize,
    pub(in crate::events) cleanup_converged_to_cold: Vec<(Millis, Millis)>,
    pub(in crate::events) cursor_color_probe_refresh_retried: usize,
    pub(in crate::events) background_probe_refresh_retried: usize,
    pub(in crate::events) cursor_color_probe_refresh_budget_exhausted: usize,
    pub(in crate::events) background_probe_refresh_budget_exhausted: usize,
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

#[derive(Debug, Default)]
pub(in crate::events) struct ScheduledEffectQueueState {
    pub(in crate::events) items: VecDeque<ScheduledWorkItem>,
    pub(in crate::events) pending_work_units: usize,
    pub(in crate::events) drain_scheduled: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(in crate::events) struct ScheduledStageResult {
    pub(in crate::events) should_schedule: bool,
    pub(in crate::events) depth: usize,
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

    pub(in crate::events) fn stage_batch(&mut self, effects: Vec<Effect>) -> ScheduledStageResult {
        if effects.is_empty() {
            return ScheduledStageResult {
                should_schedule: false,
                depth: self.pending_work_units,
            };
        }

        for segment in split_effect_batch_by_semantic_power(effects) {
            match segment {
                EffectBatchSegment::Ordered(effects) => {
                    self.items
                        .push_back(ScheduledWorkItem::OrderedEffectBatch(effects));
                    self.pending_work_units = self.pending_work_units.saturating_add(1);
                }
                EffectBatchSegment::ShellOnly(effects) => {
                    // CONTEXT: Only shell-only telemetry/redraw work may be folded into an agenda.
                    // Timer, cleanup, and other host-feedback effects stay in ordered batches so
                    // callbacks that can reach the reducer cannot be coalesced by queue policy.
                    let added_work_units = match self.items.back_mut() {
                        Some(ScheduledWorkItem::ShellOnlyAgenda(agenda)) => {
                            agenda.append_effects(effects)
                        }
                        _ => {
                            let mut agenda = ShellOnlyAgenda::default();
                            let added_work_units = agenda.append_effects(effects);
                            self.items
                                .push_back(ScheduledWorkItem::ShellOnlyAgenda(agenda));
                            added_work_units
                        }
                    };
                    self.pending_work_units =
                        self.pending_work_units.saturating_add(added_work_units);
                }
            }
        }
        self.finish_stage()
    }

    pub(in crate::events) fn stage_core_event(&mut self, event: CoreEvent) -> ScheduledStageResult {
        self.stage_irreducible_item(ScheduledWorkItem::CoreEvent(Box::new(event)))
    }

    pub(in crate::events) fn pop_work_unit(&mut self) -> Option<ScheduledWorkUnit> {
        let unit = match self.items.front()? {
            ScheduledWorkItem::OrderedEffectBatch(_) => match self.items.pop_front()? {
                ScheduledWorkItem::OrderedEffectBatch(effects) => {
                    ScheduledWorkUnit::OrderedEffectBatch(effects)
                }
                _ => unreachable!("front queue item should stay stable while borrowed"),
            },
            ScheduledWorkItem::CoreEvent(_) => match self.items.pop_front()? {
                ScheduledWorkItem::CoreEvent(event) => ScheduledWorkUnit::CoreEvent(event),
                _ => unreachable!("front queue item should stay stable while borrowed"),
            },
            ScheduledWorkItem::ShellOnlyAgenda(_) => {
                let (step, agenda_is_empty) = match self.items.front_mut() {
                    Some(ScheduledWorkItem::ShellOnlyAgenda(agenda)) => {
                        let Some(step) = agenda.pop_step() else {
                            unreachable!("non-empty shell-only agenda should yield a step");
                        };
                        (step, agenda.is_empty())
                    }
                    _ => unreachable!("front queue item should stay stable while borrowed"),
                };
                if agenda_is_empty {
                    let _ = self.items.pop_front();
                }
                ScheduledWorkUnit::ShellOnlyStep(step)
            }
        };
        self.pending_work_units = self.pending_work_units.saturating_sub(1);
        Some(unit)
    }

    pub(in crate::events) fn reset(&mut self) {
        *self = Self::default();
    }
}

fn split_effect_batch_by_semantic_power(effects: Vec<Effect>) -> Vec<EffectBatchSegment> {
    let mut segments = Vec::new();
    for effect in effects {
        match effect.into_ordered_or_shell_only() {
            Ok(effect) => append_ordered_effect(&mut segments, effect),
            Err(effect) => append_shell_only_effect(&mut segments, effect),
        }
    }
    segments
}

fn append_ordered_effect(segments: &mut Vec<EffectBatchSegment>, effect: OrderedEffect) {
    match segments.last_mut() {
        Some(EffectBatchSegment::Ordered(effects)) => effects.push(effect),
        Some(EffectBatchSegment::ShellOnly(_)) | None => {
            segments.push(EffectBatchSegment::Ordered(vec![effect]));
        }
    }
}

fn append_shell_only_effect(segments: &mut Vec<EffectBatchSegment>, effect: ShellOnlyEffect) {
    match segments.last_mut() {
        Some(EffectBatchSegment::ShellOnly(effects)) => effects.push(effect),
        Some(EffectBatchSegment::Ordered(_)) | None => {
            segments.push(EffectBatchSegment::ShellOnly(vec![effect]));
        }
    }
}
