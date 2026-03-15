use super::super::logging::{trace_lazy, warn};
use crate::core::effect::RequestRenderPlanEffect;
use crate::core::event::{Event as CoreEvent, RenderPlanComputedEvent, RenderPlanFailedEvent};
use crate::core::reducer::build_planned_render;

pub(crate) fn execute_core_request_render_plan_effect(
    payload: &RequestRenderPlanEffect,
) -> Vec<CoreEvent> {
    trace_lazy(|| {
        format!(
            "render_plan_start proposal_id={} observation_id={} requested_at={}",
            payload.proposal_id.value(),
            payload.observation.basis().observation_id().value(),
            payload.requested_at.value(),
        )
    });

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        build_planned_render(
            &payload.planning_state,
            payload.proposal_id,
            &payload.render_decision,
            payload.animation_schedule,
        )
    }));

    let follow_up = outcome.map_or_else(
        |_| {
            warn("core render planning panicked");
            CoreEvent::RenderPlanFailed(RenderPlanFailedEvent {
                proposal_id: payload.proposal_id,
                observed_at: payload.requested_at,
            })
        },
        |planned_render| {
            CoreEvent::RenderPlanComputed(RenderPlanComputedEvent {
                proposal_id: payload.proposal_id,
                planned_render: Box::new(planned_render),
                observed_at: payload.requested_at,
            })
        },
    );

    trace_lazy(|| {
        format!(
            "render_plan_result proposal_id={} result={}",
            payload.proposal_id.value(),
            match &follow_up {
                CoreEvent::RenderPlanComputed(_) => "computed",
                CoreEvent::RenderPlanFailed(_) => "failed",
                _ => "unexpected",
            }
        )
    });

    vec![follow_up]
}
