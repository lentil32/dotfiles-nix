use super::*;
use crate::core::effect::EventLoopMetricEffect;
use crate::core::effect::IngressCursorPresentationEffect;
use crate::core::effect::OrderedEffect;
use crate::core::effect::ScheduleTimerEffect;
use crate::core::effect::ShellOnlyEffect;
use crate::core::state::ProbeKind;
use crate::core::state::RenderCleanupState;
use crate::core::state::RenderThermalState;
use crate::core::types::DelayBudgetMs;
use crate::core::types::TimerGeneration;
use crate::core::types::TimerId;
use crate::core::types::TimerToken;
use crate::events::runtime::MAX_CHAINED_SCHEDULED_WORK_ITEMS_PER_EDGE;
use crate::test_support::proptest::timer_id;
use proptest::collection::vec;
use proptest::prelude::*;
use std::collections::VecDeque;

pub(super) struct ScheduledDrainHarness {
    _scope: CoreDispatchTestContext,
}

impl ScheduledDrainHarness {
    pub(super) fn new() -> Self {
        Self {
            _scope: CoreDispatchTestContext::new(),
        }
    }

    pub(super) fn with_cleanup_thermal(thermal: RenderThermalState) -> Self {
        let harness = Self::new();
        replace_core_state(ready_state_with_cleanup_thermal(thermal));
        harness
    }

    pub(super) fn stage_batch(&self, effects: Vec<Effect>) -> bool {
        queue_stage_batch(effects)
    }

    pub(super) fn stage_two_effect_batches(&self) {
        self.stage_effect_batches(
            [vec![Effect::RedrawCmdline], vec![cleanup_timer_effect(1)]],
            "two-effect snapshot",
        );
    }

    pub(super) fn stage_redraw_waves(&self, count: usize) {
        self.stage_effect_batches(
            (0..count).map(|_| vec![Effect::RedrawCmdline]),
            "redraw waves",
        );
    }

    pub(super) fn queued_work_count(&self) -> usize {
        queued_work_count()
    }

    pub(super) fn queued_front_work_item(&self) -> Option<ScheduledWorkUnit> {
        queued_front_work_item()
    }

    pub(super) fn queue_is_marked_scheduled(&self) -> bool {
        queue_is_marked_scheduled()
    }

    pub(super) fn drain_next_edge(&self, executor: &mut RecordingExecutor) -> bool {
        drain_next_edge(executor)
    }

    fn stage_effect_batches<I>(&self, batches: I, label: &str)
    where
        I: IntoIterator<Item = Vec<Effect>>,
    {
        for (index, effects) in batches.into_iter().enumerate() {
            let expected_schedule = queued_work_count() == 0 && !queue_is_marked_scheduled();
            let should_schedule = queue_stage_batch(effects);
            assert_eq!(
                should_schedule,
                expected_schedule,
                "{label} batch {index} should {} the drain edge",
                if expected_schedule { "arm" } else { "reuse" },
            );
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum ScheduledDrainMetricSpec {
    IngressCoalesced,
    DelayedIngressPendingUpdated,
    StaleToken,
    ProbeRefreshRetried(ProbeKind),
    ProbeRefreshBudgetExhausted(ProbeKind),
}

impl ScheduledDrainMetricSpec {
    fn effect(self) -> EventLoopMetricEffect {
        match self {
            Self::IngressCoalesced => EventLoopMetricEffect::IngressCoalesced,
            Self::DelayedIngressPendingUpdated => {
                EventLoopMetricEffect::DelayedIngressPendingUpdated
            }
            Self::StaleToken => EventLoopMetricEffect::StaleToken,
            Self::ProbeRefreshRetried(kind) => EventLoopMetricEffect::ProbeRefreshRetried(kind),
            Self::ProbeRefreshBudgetExhausted(kind) => {
                EventLoopMetricEffect::ProbeRefreshBudgetExhausted(kind)
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum ScheduledDrainEffectSpec {
    Cleanup { max_kept_windows: usize },
    Timer { timer_id: TimerId, generation: u64 },
    Metric(ScheduledDrainMetricSpec),
    RedrawCmdline,
    NonCoalescible,
}

impl ScheduledDrainEffectSpec {
    fn effect(self) -> Effect {
        match self {
            Self::Cleanup { max_kept_windows } => cleanup_effect(max_kept_windows),
            Self::Timer {
                timer_id,
                generation,
            } => timer_effect(timer_id, generation),
            Self::Metric(metric) => Effect::RecordEventLoopMetric(metric.effect()),
            Self::RedrawCmdline => Effect::RedrawCmdline,
            Self::NonCoalescible => non_coalescible_effect(),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum ScheduledDrainOperation {
    Stage(Vec<ScheduledDrainEffectSpec>),
    Drain,
}

fn scheduled_drain_probe_kind() -> impl Strategy<Value = ProbeKind> {
    prop_oneof![Just(ProbeKind::CursorColor), Just(ProbeKind::Background)]
}

fn scheduled_drain_metric_spec() -> BoxedStrategy<ScheduledDrainMetricSpec> {
    prop_oneof![
        Just(ScheduledDrainMetricSpec::IngressCoalesced),
        Just(ScheduledDrainMetricSpec::DelayedIngressPendingUpdated),
        Just(ScheduledDrainMetricSpec::StaleToken),
        scheduled_drain_probe_kind().prop_map(ScheduledDrainMetricSpec::ProbeRefreshRetried),
        scheduled_drain_probe_kind()
            .prop_map(ScheduledDrainMetricSpec::ProbeRefreshBudgetExhausted),
    ]
    .boxed()
}

fn scheduled_drain_effect_spec() -> BoxedStrategy<ScheduledDrainEffectSpec> {
    prop_oneof![
        (0_usize..=24_usize)
            .prop_map(|max_kept_windows| ScheduledDrainEffectSpec::Cleanup { max_kept_windows }),
        (timer_id(), any::<u8>()).prop_map(|(timer_id, generation)| {
            ScheduledDrainEffectSpec::Timer {
                timer_id,
                generation: u64::from(generation),
            }
        }),
        scheduled_drain_metric_spec().prop_map(ScheduledDrainEffectSpec::Metric),
        Just(ScheduledDrainEffectSpec::RedrawCmdline),
        Just(ScheduledDrainEffectSpec::NonCoalescible),
    ]
    .boxed()
}

pub(super) fn scheduled_drain_operation() -> BoxedStrategy<ScheduledDrainOperation> {
    prop_oneof![
        vec(scheduled_drain_effect_spec(), 1..=4).prop_map(ScheduledDrainOperation::Stage),
        Just(ScheduledDrainOperation::Drain),
    ]
    .boxed()
}

pub(super) fn scheduled_drain_thermal() -> BoxedStrategy<RenderThermalState> {
    prop_oneof![
        Just(RenderThermalState::Hot),
        Just(RenderThermalState::Cooling),
        Just(RenderThermalState::Cold),
    ]
    .boxed()
}

pub(super) fn scheduled_drain_effects(specs: &[ScheduledDrainEffectSpec]) -> Vec<Effect> {
    specs
        .iter()
        .copied()
        .map(ScheduledDrainEffectSpec::effect)
        .collect()
}

#[derive(Debug, PartialEq)]
pub(super) struct ScheduledDrainModelOutcome {
    pub(super) executed_effects: Vec<Effect>,
    pub(super) has_more_items: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum ScheduledDrainModelItem {
    OrderedEffectBatch(Vec<OrderedEffect>),
    ShellOnlyAgenda(ScheduledDrainModelAgenda),
}

#[derive(Clone, Debug, Default, PartialEq)]
struct ScheduledDrainModelAgenda {
    steps: VecDeque<ScheduledDrainModelStep>,
}

impl ScheduledDrainModelAgenda {
    fn append_effects(&mut self, effects: Vec<ShellOnlyEffect>) -> usize {
        effects
            .into_iter()
            .map(|effect| usize::from(self.append_effect(effect)))
            .sum()
    }

    fn append_effect(&mut self, effect: ShellOnlyEffect) -> bool {
        match effect {
            ShellOnlyEffect::RecordEventLoopMetric(_) => {
                if matches!(
                    self.steps.back(),
                    Some(ScheduledDrainModelStep::RecordMetrics)
                ) {
                    return false;
                }
                self.steps.push_back(ScheduledDrainModelStep::RecordMetrics);
                true
            }
            ShellOnlyEffect::RedrawCmdline => {
                if matches!(
                    self.steps.back(),
                    Some(ScheduledDrainModelStep::RedrawCmdline)
                ) {
                    return false;
                }
                self.steps.push_back(ScheduledDrainModelStep::RedrawCmdline);
                true
            }
        }
    }

    fn pop_step(&mut self) -> Option<ScheduledDrainModelStep> {
        self.steps.pop_front()
    }

    fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ScheduledDrainModelStep {
    RecordMetrics,
    RedrawCmdline,
}

impl ScheduledDrainModelStep {
    fn execute(self, executed_effects: &mut Vec<Effect>) {
        match self {
            Self::RecordMetrics => {}
            Self::RedrawCmdline => {
                executed_effects.push(Effect::RedrawCmdline);
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct ScheduledDrainModel {
    thermal: RenderThermalState,
    items: VecDeque<ScheduledDrainModelItem>,
    pending_work_units: usize,
    drain_scheduled: bool,
}

impl ScheduledDrainModel {
    pub(super) fn new(thermal: RenderThermalState) -> Self {
        Self {
            thermal,
            items: VecDeque::new(),
            pending_work_units: 0,
            drain_scheduled: false,
        }
    }

    pub(super) fn stage_batch(&mut self, effects: Vec<Effect>) -> bool {
        if effects.is_empty() {
            return false;
        }

        for segment in split_effect_batch_by_semantic_power(effects) {
            match segment {
                ScheduledDrainModelSegment::Ordered(effects) => {
                    self.items
                        .push_back(ScheduledDrainModelItem::OrderedEffectBatch(effects));
                    self.pending_work_units = self.pending_work_units.saturating_add(1);
                }
                ScheduledDrainModelSegment::ShellOnly(effects) => {
                    let added_work_units = match self.items.back_mut() {
                        Some(ScheduledDrainModelItem::ShellOnlyAgenda(agenda)) => {
                            agenda.append_effects(effects)
                        }
                        _ => {
                            let mut agenda = ScheduledDrainModelAgenda::default();
                            let added_work_units = agenda.append_effects(effects);
                            self.items
                                .push_back(ScheduledDrainModelItem::ShellOnlyAgenda(agenda));
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

    pub(super) fn queued_work_count(&self) -> usize {
        self.pending_work_units
    }

    pub(super) fn queue_is_marked_scheduled(&self) -> bool {
        self.drain_scheduled
    }

    pub(super) fn drain_next_edge(&mut self) -> ScheduledDrainModelOutcome {
        let mut executed_effects = Vec::new();
        let mut drained_items = 0_usize;

        loop {
            let snapshot_budget = self.scheduled_drain_budget();
            let mut remaining_budget = snapshot_budget;
            let mut drained_items_this_pass = 0_usize;

            while remaining_budget > 0 {
                let Some(work_unit) = self.pop_work_unit() else {
                    self.drain_scheduled = false;
                    return ScheduledDrainModelOutcome {
                        executed_effects,
                        has_more_items: false,
                    };
                };

                match work_unit {
                    ScheduledDrainModelWorkUnit::OrderedEffectBatch(effects) => {
                        executed_effects.extend(effects.into_iter().map(Effect::from));
                    }
                    ScheduledDrainModelWorkUnit::ShellOnlyStep(step) => {
                        step.execute(&mut executed_effects);
                    }
                }

                drained_items = drained_items.saturating_add(1);
                drained_items_this_pass = drained_items_this_pass.saturating_add(1);
                remaining_budget -= 1;
            }

            if !self.should_continue_hot_shell_only_tail(
                snapshot_budget,
                drained_items_this_pass,
                drained_items,
            ) {
                break;
            }
        }

        let has_more_items = !self.items.is_empty();
        if !has_more_items {
            self.drain_scheduled = false;
        }

        ScheduledDrainModelOutcome {
            executed_effects,
            has_more_items,
        }
    }

    fn finish_stage(&mut self) -> bool {
        if self.drain_scheduled {
            false
        } else {
            self.drain_scheduled = true;
            true
        }
    }

    fn scheduled_drain_budget(&mut self) -> usize {
        if self.pending_work_units == 0 {
            self.drain_scheduled = false;
            return 0;
        }

        if self.thermal == RenderThermalState::Hot && self.has_only_shell_only_agendas() {
            return scheduled_drain_budget_for_hot_effect_only_snapshot(self.pending_work_units);
        }

        scheduled_drain_budget_for_thermal(self.thermal, self.pending_work_units)
    }

    fn has_only_shell_only_agendas(&self) -> bool {
        !self.items.is_empty()
            && self
                .items
                .iter()
                .all(|item| matches!(item, ScheduledDrainModelItem::ShellOnlyAgenda(_)))
    }

    fn should_continue_hot_shell_only_tail(
        &self,
        snapshot_budget: usize,
        drained_items_this_pass: usize,
        drained_items_total: usize,
    ) -> bool {
        self.thermal == RenderThermalState::Hot
            && drained_items_this_pass == snapshot_budget
            && drained_items_total < MAX_CHAINED_SCHEDULED_WORK_ITEMS_PER_EDGE
            && self.has_only_shell_only_agendas()
    }

    fn pop_work_unit(&mut self) -> Option<ScheduledDrainModelWorkUnit> {
        let work_unit = match self.items.front()? {
            ScheduledDrainModelItem::OrderedEffectBatch(_) => match self.items.pop_front()? {
                ScheduledDrainModelItem::OrderedEffectBatch(effects) => {
                    ScheduledDrainModelWorkUnit::OrderedEffectBatch(effects)
                }
                ScheduledDrainModelItem::ShellOnlyAgenda(_) => {
                    unreachable!("front model work item should stay stable while borrowed")
                }
            },
            ScheduledDrainModelItem::ShellOnlyAgenda(_) => {
                let (step, agenda_is_empty) = match self.items.front_mut() {
                    Some(ScheduledDrainModelItem::ShellOnlyAgenda(agenda)) => {
                        let Some(step) = agenda.pop_step() else {
                            unreachable!("non-empty model agenda should yield a step");
                        };
                        (step, agenda.is_empty())
                    }
                    Some(ScheduledDrainModelItem::OrderedEffectBatch(_)) => {
                        unreachable!("front model work item should stay stable while borrowed")
                    }
                    None => unreachable!("front model work item should still exist while borrowed"),
                };
                if agenda_is_empty {
                    let _ = self.items.pop_front();
                }
                ScheduledDrainModelWorkUnit::ShellOnlyStep(step)
            }
        };
        self.pending_work_units = self.pending_work_units.saturating_sub(1);
        Some(work_unit)
    }
}

#[derive(Debug)]
enum ScheduledDrainModelWorkUnit {
    OrderedEffectBatch(Vec<OrderedEffect>),
    ShellOnlyStep(ScheduledDrainModelStep),
}

#[derive(Debug)]
enum ScheduledDrainModelSegment {
    Ordered(Vec<OrderedEffect>),
    ShellOnly(Vec<ShellOnlyEffect>),
}

fn split_effect_batch_by_semantic_power(effects: Vec<Effect>) -> Vec<ScheduledDrainModelSegment> {
    let mut segments = Vec::new();
    for effect in effects {
        match effect.into_ordered_or_shell_only() {
            Ok(effect) => append_ordered_effect(&mut segments, effect),
            Err(effect) => append_shell_only_effect(&mut segments, effect),
        }
    }
    segments
}

fn append_ordered_effect(segments: &mut Vec<ScheduledDrainModelSegment>, effect: OrderedEffect) {
    match segments.last_mut() {
        Some(ScheduledDrainModelSegment::Ordered(effects)) => effects.push(effect),
        Some(ScheduledDrainModelSegment::ShellOnly(_)) | None => {
            segments.push(ScheduledDrainModelSegment::Ordered(vec![effect]));
        }
    }
}

fn append_shell_only_effect(
    segments: &mut Vec<ScheduledDrainModelSegment>,
    effect: ShellOnlyEffect,
) {
    match segments.last_mut() {
        Some(ScheduledDrainModelSegment::ShellOnly(effects)) => effects.push(effect),
        Some(ScheduledDrainModelSegment::Ordered(_)) | None => {
            segments.push(ScheduledDrainModelSegment::ShellOnly(vec![effect]));
        }
    }
}

#[derive(Default)]
pub(super) struct ExecutorPlan {
    planned_follow_ups: VecDeque<Vec<CoreEvent>>,
}

impl ExecutorPlan {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn follow_up(mut self, events: Vec<CoreEvent>) -> Self {
        self.planned_follow_ups.push_back(events);
        self
    }

    pub(super) fn no_follow_up(self) -> Self {
        self.follow_up(Vec::new())
    }

    pub(super) fn build(self) -> RecordingExecutor {
        RecordingExecutor {
            executed_effects: Vec::new(),
            planned_follow_ups: self.planned_follow_ups,
        }
    }
}

pub(super) fn cleanup_effect(max_kept_windows: usize) -> Effect {
    Effect::ApplyRenderCleanup(crate::core::effect::ApplyRenderCleanupEffect {
        execution: crate::core::effect::RenderCleanupExecution::SoftClear { max_kept_windows },
    })
}

pub(super) fn cleanup_timer_effect(generation: u64) -> Effect {
    schedule_timer_effect(TimerId::Cleanup, generation)
}

pub(super) fn non_coalescible_effect() -> Effect {
    Effect::ApplyIngressCursorPresentation(IngressCursorPresentationEffect::HideCursor)
}

fn ready_state_with_cleanup_thermal(thermal: RenderThermalState) -> CoreState {
    ready_state().with_render_cleanup(render_cleanup_for_thermal(thermal))
}

fn render_cleanup_for_thermal(thermal: RenderThermalState) -> RenderCleanupState {
    let scheduled = RenderCleanupState::scheduled(Millis::new(40), 25, 90);
    match thermal {
        RenderThermalState::Hot => scheduled,
        RenderThermalState::Cooling => scheduled.enter_cooling(Millis::new(65)),
        RenderThermalState::Cold => RenderCleanupState::cold(),
    }
}

pub(super) fn timer_effect(timer_id: TimerId, generation: u64) -> Effect {
    Effect::ScheduleTimer(ScheduleTimerEffect {
        token: TimerToken::new(timer_id, TimerGeneration::new(generation)),
        delay: DelayBudgetMs::try_new(1).expect("positive timer delay"),
        requested_at: Millis::new(generation),
    })
}

fn schedule_timer_effect(timer_id: TimerId, generation: u64) -> Effect {
    timer_effect(timer_id, generation)
}
