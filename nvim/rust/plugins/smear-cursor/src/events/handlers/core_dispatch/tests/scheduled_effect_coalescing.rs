use super::scheduled_effect_drain_support::ScheduledDrainHarness;
use super::scheduled_effect_drain_support::cleanup_timer_effect;
use super::*;
use crate::core::effect::EventLoopMetricEffect;
use crate::core::state::ProbeKind;
use crate::events::runtime::ScheduledEffectQueueState;
use crate::events::runtime::ShellOnlyStep;
use pretty_assertions::assert_eq;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct RuntimeMetricObservation {
    ingress_coalesced: u64,
    delayed_ingress_pending_updates: u64,
    stale_token_events: u64,
    post_burst_convergence_samples: u64,
    post_burst_convergence_total_ms: u64,
    post_burst_convergence_max_ms: u64,
    post_burst_convergence_last_ms: u64,
    cursor_color_probe_refresh_retries: u64,
    background_probe_refresh_retries: u64,
    cursor_color_probe_refresh_budget_exhausted: u64,
    background_probe_refresh_budget_exhausted: u64,
}

impl RuntimeMetricObservation {
    fn current() -> Self {
        let metrics = crate::events::event_loop::diagnostics_snapshot().metrics;
        Self {
            ingress_coalesced: metrics.ingress_coalesced,
            delayed_ingress_pending_updates: metrics.delayed_ingress_pending_updates,
            stale_token_events: metrics.stale_token_events,
            post_burst_convergence_samples: metrics.post_burst_convergence.samples,
            post_burst_convergence_total_ms: metrics.post_burst_convergence.total_ms,
            post_burst_convergence_max_ms: metrics.post_burst_convergence.max_ms,
            post_burst_convergence_last_ms: metrics.post_burst_convergence.last_ms,
            cursor_color_probe_refresh_retries: metrics.cursor_color_probe.refresh_retries,
            background_probe_refresh_retries: metrics.background_probe.refresh_retries,
            cursor_color_probe_refresh_budget_exhausted: metrics
                .cursor_color_probe
                .refresh_budget_exhausted,
            background_probe_refresh_budget_exhausted: metrics
                .background_probe
                .refresh_budget_exhausted,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
struct QueuedMetricObservation {
    ingress_coalesced: u64,
    delayed_ingress_pending_updated: u64,
    stale_token: u64,
    cursor_color_probe_refresh_retried: u64,
    background_probe_refresh_retried: u64,
    cursor_color_probe_refresh_budget_exhausted: u64,
    background_probe_refresh_budget_exhausted: u64,
}

#[derive(Debug, Clone, PartialEq)]
enum WorkUnitObservation {
    Metrics(QueuedMetricObservation),
    RedrawCmdline,
    OrderedEffects(Vec<Effect>),
}

struct EventLoopTelemetryScope;

impl EventLoopTelemetryScope {
    fn new() -> Self {
        reset_event_loop_metrics_for_test();
        Self
    }
}

impl Drop for EventLoopTelemetryScope {
    fn drop(&mut self) {
        reset_event_loop_metrics_for_test();
    }
}

fn reset_event_loop_metrics_for_test() {
    crate::events::event_loop::reset_for_test();
}

fn metric_effects_for_coalescing_equivalence() -> Vec<EventLoopMetricEffect> {
    vec![
        EventLoopMetricEffect::IngressCoalesced,
        EventLoopMetricEffect::DelayedIngressPendingUpdated,
        EventLoopMetricEffect::ProbeRefreshRetried(ProbeKind::CursorColor),
        EventLoopMetricEffect::ProbeRefreshRetried(ProbeKind::Background),
        EventLoopMetricEffect::ProbeRefreshBudgetExhausted(ProbeKind::CursorColor),
        EventLoopMetricEffect::ProbeRefreshBudgetExhausted(ProbeKind::Background),
        EventLoopMetricEffect::CleanupConvergedToCold {
            started_at: Millis::new(/*value*/ 10),
            converged_at: Millis::new(/*value*/ 18),
        },
        EventLoopMetricEffect::StaleToken,
        EventLoopMetricEffect::IngressCoalesced,
    ]
}

fn record_metric_effect_directly(metric: EventLoopMetricEffect) {
    match metric {
        EventLoopMetricEffect::IngressCoalesced => {
            crate::events::event_loop::record_ingress_coalesced();
        }
        EventLoopMetricEffect::DelayedIngressPendingUpdated => {
            crate::events::event_loop::record_delayed_ingress_pending_update();
        }
        EventLoopMetricEffect::CleanupConvergedToCold {
            started_at,
            converged_at,
        } => {
            crate::events::event_loop::record_post_burst_convergence(started_at, converged_at);
        }
        EventLoopMetricEffect::StaleToken => {
            crate::events::event_loop::record_stale_token_event();
        }
        EventLoopMetricEffect::ProbeRefreshRetried(kind) => {
            crate::events::event_loop::record_probe_refresh_retried(kind);
        }
        EventLoopMetricEffect::ProbeRefreshBudgetExhausted(kind) => {
            crate::events::event_loop::record_probe_refresh_budget_exhausted(kind);
        }
    }
}

fn stage_metric_effects(harness: &ScheduledDrainHarness, metrics: &[EventLoopMetricEffect]) {
    for (index, metric) in metrics.iter().copied().enumerate() {
        assert_eq!(
            harness.stage_batch(vec![Effect::RecordEventLoopMetric(metric)]),
            index == 0,
            "only the first metric batch should arm the drain"
        );
    }
}

fn queued_metric_observation(
    metrics: crate::events::runtime::PendingMetricEffects,
) -> WorkUnitObservation {
    WorkUnitObservation::Metrics(QueuedMetricObservation {
        ingress_coalesced: metrics.ingress_coalesced as u64,
        delayed_ingress_pending_updated: metrics.delayed_ingress_pending_updated as u64,
        stale_token: metrics.stale_token as u64,
        cursor_color_probe_refresh_retried: metrics.cursor_color_probe_refresh_retried as u64,
        background_probe_refresh_retried: metrics.background_probe_refresh_retried as u64,
        cursor_color_probe_refresh_budget_exhausted: metrics
            .cursor_color_probe_refresh_budget_exhausted
            as u64,
        background_probe_refresh_budget_exhausted: metrics.background_probe_refresh_budget_exhausted
            as u64,
    })
}

fn pop_work_unit_observations() -> Vec<WorkUnitObservation> {
    let mut observations = Vec::new();
    while let Some(work_unit) = with_dispatch_queue(ScheduledEffectQueueState::pop_work_unit) {
        observations.push(match work_unit {
            ScheduledWorkUnit::OrderedEffectBatch(effects) => {
                WorkUnitObservation::OrderedEffects(effects.into_iter().map(Effect::from).collect())
            }
            ScheduledWorkUnit::CoreEvent(_) => {
                unreachable!("coalescing tests do not stage core event work units")
            }
            ScheduledWorkUnit::ShellOnlyStep(ShellOnlyStep::RecordMetrics(metrics)) => {
                queued_metric_observation(metrics)
            }
            ScheduledWorkUnit::ShellOnlyStep(ShellOnlyStep::RedrawCmdline) => {
                WorkUnitObservation::RedrawCmdline
            }
        });
    }
    observations
}

#[test]
fn coalesced_metric_shell_only_effects_match_uncoalesced_metric_recording() {
    let _telemetry = EventLoopTelemetryScope::new();
    let metric_effects = metric_effects_for_coalescing_equivalence();
    for metric in metric_effects.iter().copied() {
        record_metric_effect_directly(metric);
    }
    let expected_metrics = RuntimeMetricObservation::current();

    let harness = ScheduledDrainHarness::new();
    reset_event_loop_metrics_for_test();
    let initial_core_state = current_core_state();
    stage_metric_effects(&harness, &metric_effects);

    assert_eq!(
        harness.queued_work_count(),
        1,
        "adjacent metric effects should aggregate into one shell-only work unit"
    );

    let mut executor = RecordingExecutor::default();
    assert!(!harness.drain_next_edge(&mut executor));

    assert_eq!(executor.executed_effects, Vec::<Effect>::new());
    assert_eq!(current_core_state(), initial_core_state);
    assert_eq!(RuntimeMetricObservation::current(), expected_metrics);
}

#[test]
fn ordered_effects_remain_fifo_barriers_around_shell_only_coalescing() {
    let harness = ScheduledDrainHarness::new();
    let first_timer = cleanup_timer_effect(/*generation*/ 1);
    let second_timer = cleanup_timer_effect(/*generation*/ 2);

    assert!(harness.stage_batch(vec![
        Effect::RecordEventLoopMetric(EventLoopMetricEffect::IngressCoalesced),
        first_timer.clone(),
        Effect::RecordEventLoopMetric(EventLoopMetricEffect::DelayedIngressPendingUpdated),
    ]));
    assert!(!harness.stage_batch(vec![
        Effect::RecordEventLoopMetric(EventLoopMetricEffect::StaleToken),
        second_timer.clone(),
        Effect::RedrawCmdline,
    ]));

    assert_eq!(
        pop_work_unit_observations(),
        vec![
            WorkUnitObservation::Metrics(QueuedMetricObservation {
                ingress_coalesced: 1,
                ..QueuedMetricObservation::default()
            }),
            WorkUnitObservation::OrderedEffects(vec![first_timer]),
            WorkUnitObservation::Metrics(QueuedMetricObservation {
                delayed_ingress_pending_updated: 1,
                stale_token: 1,
                ..QueuedMetricObservation::default()
            }),
            WorkUnitObservation::OrderedEffects(vec![second_timer]),
            WorkUnitObservation::RedrawCmdline,
        ]
    );
}
