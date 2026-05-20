use super::super::handlers;
use super::super::host_bridge::InstalledHostBridge;
use super::super::host_bridge::installed_host_bridge;
use super::super::logging::trace_lazy;
use super::telemetry::note_observation_request_now;
use super::telemetry::record_observation_request_executed;
use super::telemetry::record_post_burst_convergence;
use super::telemetry::record_probe_duration;
use super::telemetry::record_probe_refresh_budget_exhausted;
use super::telemetry::record_probe_refresh_retried;
use super::telemetry::record_stale_token_event;
use super::timers::duration_to_micros;
use super::timers::now_ms;
use super::timers::resolved_timer_delay_ms;
use super::timers::schedule_core_timer_effect;
use super::to_core_millis;
use crate::core::effect::Effect;
use crate::core::effect::EventLoopMetricEffect;
use crate::core::effect::RequestProbeEffect;
use crate::core::event::EffectFailedEvent;
use crate::core::event::EffectFailureSource;
use crate::core::event::Event;
use crate::events::handlers::ProbeDispatchWave;
use nvim_oxi::Result as NvimResult;
use std::fmt;
use std::time::Instant;

pub(crate) fn record_effect_failure(source: EffectFailureSource, context: &'static str) {
    let observed_at = to_core_millis(now_ms());
    trace_lazy(|| {
        format!(
            "effect_failure_recorded source={source:?} context={context} observed_at={}",
            observed_at.value(),
        )
    });
    handlers::stage_core_event_with_default_scheduler(Event::EffectFailed(EffectFailedEvent {
        proposal_id: None,
        observed_at,
    }));
}

pub(crate) type EffectExecutionResult<T> = std::result::Result<T, EffectExecutionError>;

#[derive(Debug)]
pub(crate) struct EffectExecutionError {
    source: nvim_oxi::Error,
    kind: EffectExecutionFailureKind,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum EffectExecutionFailureKind {
    Reportable,
    BenignObservationCancellation {
        reason: ObservationCancellationReason,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ObservationCancellationReason {
    CurrentWindow,
    CurrentBuffer,
    CurrentViewport,
    Surface,
}

impl EffectExecutionError {
    pub(crate) fn reportable(source: nvim_oxi::Error) -> Self {
        Self {
            source,
            kind: EffectExecutionFailureKind::Reportable,
        }
    }

    pub(crate) fn benign_observation_cancellation(
        source: nvim_oxi::Error,
        reason: ObservationCancellationReason,
    ) -> Self {
        Self {
            source,
            kind: EffectExecutionFailureKind::BenignObservationCancellation { reason },
        }
    }

    pub(crate) fn kind(&self) -> EffectExecutionFailureKind {
        self.kind
    }

    pub(crate) fn should_report_scheduled_work_failure(&self) -> bool {
        self.kind == EffectExecutionFailureKind::Reportable
    }
}

impl fmt::Display for EffectExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(f)
    }
}

impl From<nvim_oxi::Error> for EffectExecutionError {
    fn from(error: nvim_oxi::Error) -> Self {
        Self::reportable(error)
    }
}

pub(crate) trait EffectExecutor {
    fn execute_effect(&mut self, effect: Effect) -> EffectExecutionResult<Vec<Event>>;

    fn execute_probe_effect(
        &mut self,
        payload: RequestProbeEffect,
        _dispatch_wave: ProbeDispatchWave,
    ) -> EffectExecutionResult<Vec<Event>> {
        self.execute_effect(Effect::RequestProbe(payload))
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct NeovimEffectExecutor {
    host_bridge: InstalledHostBridge,
}

impl NeovimEffectExecutor {
    pub(crate) fn new() -> NvimResult<Self> {
        Ok(Self {
            host_bridge: installed_host_bridge()?,
        })
    }
}

impl EffectExecutor for NeovimEffectExecutor {
    fn execute_probe_effect(
        &mut self,
        payload: RequestProbeEffect,
        dispatch_wave: ProbeDispatchWave,
    ) -> EffectExecutionResult<Vec<Event>> {
        let kind = payload.kind;
        let started_at = Instant::now();
        let result = match dispatch_wave {
            ProbeDispatchWave::NewReducerWave => {
                handlers::execute_core_request_probe_effect(&payload)
            }
            ProbeDispatchWave::SameReducerWave => {
                handlers::execute_core_request_probe_effect_same_reducer_wave(&payload)
            }
        };
        record_probe_duration(kind, duration_to_micros(started_at.elapsed()));
        Ok(result)
    }

    fn execute_effect(&mut self, effect: Effect) -> EffectExecutionResult<Vec<Event>> {
        match effect {
            Effect::ScheduleTimer(payload) => Ok(schedule_core_timer_effect(
                self.host_bridge,
                payload.token,
                resolved_timer_delay_ms(payload.token.id(), payload.delay),
                payload.requested_at,
            )),
            Effect::RequestObservationBase(payload) => {
                note_observation_request_now();
                record_observation_request_executed();
                handlers::execute_core_request_observation_base_effect(payload)
            }
            Effect::RequestProbe(payload) => {
                self.execute_probe_effect(payload, ProbeDispatchWave::NewReducerWave)
            }
            Effect::RequestRenderPlan(payload) => {
                Ok(handlers::execute_core_request_render_plan_effect(*payload))
            }
            Effect::ApplyProposal(payload) => {
                Ok(handlers::execute_core_apply_proposal_effect(*payload))
            }
            Effect::ApplyRenderCleanup(payload) => {
                Ok(handlers::execute_core_apply_render_cleanup_effect(payload))
            }
            Effect::ApplyIngressCursorPresentation(payload) => {
                Ok(handlers::apply_ingress_cursor_presentation_effect(payload))
            }
            Effect::RecordEventLoopMetric(metric) => {
                match metric {
                    EventLoopMetricEffect::IngressCoalesced => super::record_ingress_coalesced(),
                    EventLoopMetricEffect::DelayedIngressPendingUpdated => {
                        super::record_delayed_ingress_pending_update();
                    }
                    EventLoopMetricEffect::CleanupConvergedToCold {
                        started_at,
                        converged_at,
                    } => {
                        record_post_burst_convergence(started_at, converged_at);
                    }
                    EventLoopMetricEffect::StaleToken => record_stale_token_event(),
                    EventLoopMetricEffect::ProbeRefreshRetried(kind) => {
                        record_probe_refresh_retried(kind);
                    }
                    EventLoopMetricEffect::ProbeRefreshBudgetExhausted(kind) => {
                        record_probe_refresh_budget_exhausted(kind);
                    }
                }
                Ok(Vec::new())
            }
            Effect::RedrawCmdline => {
                handlers::execute_redraw_cmdline_effect();
                Ok(Vec::new())
            }
        }
    }
}
