use crate::core::effect::{Effect, EventLoopMetricEffect};
use crate::core::event::Event as CoreEvent;

pub(super) fn core_event_label(event: &CoreEvent) -> &'static str {
    match event {
        CoreEvent::Initialize(_) => "initialize",
        CoreEvent::ExternalDemandQueued(_) => "external_demand_queued",
        CoreEvent::ObservationBaseCollected(_) => "observation_base_collected",
        CoreEvent::ProbeReported(_) => "probe_reported",
        CoreEvent::RenderPlanComputed(_) => "render_plan_computed",
        CoreEvent::RenderPlanFailed(_) => "render_plan_failed",
        CoreEvent::ApplyReported(_) => "apply_reported",
        CoreEvent::RenderCleanupApplied(_) => "render_cleanup_applied",
        CoreEvent::TimerFiredWithToken(_) => "timer_fired_with_token",
        CoreEvent::TimerLostWithToken(_) => "timer_lost_with_token",
        CoreEvent::EffectFailed(_) => "effect_failed",
    }
}

pub(super) fn effect_label(effect: &Effect) -> &'static str {
    match effect {
        Effect::ScheduleTimer(_) => "schedule_timer",
        Effect::RequestObservationBase(_) => "request_observation_base",
        Effect::RequestProbe(_) => "request_probe",
        Effect::RequestRenderPlan(_) => "request_render_plan",
        Effect::ApplyProposal(_) => "apply_proposal",
        Effect::ApplyRenderCleanup(_) => "apply_render_cleanup",
        Effect::ApplyIngressCursorPresentation(_) => "apply_ingress_cursor_presentation",
        Effect::RecordEventLoopMetric(metric) => match metric {
            EventLoopMetricEffect::IngressCoalesced => "record_ingress_coalesced_metric",
            EventLoopMetricEffect::StaleToken => "record_stale_token_metric",
            EventLoopMetricEffect::ProbeRefreshRetried(_) => "record_probe_refresh_retried_metric",
            EventLoopMetricEffect::ProbeRefreshBudgetExhausted(_) => {
                "record_probe_refresh_budget_exhausted_metric"
            }
        },
        Effect::RedrawCmdline => "redraw_cmdline",
    }
}
