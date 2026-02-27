use super::super::logging::{trace_lazy, warn};
use crate::core::effect::RequestRenderPlanEffect;
use crate::core::event::{Event as CoreEvent, RenderPlanComputedEvent, RenderPlanFailedEvent};
use crate::core::reducer::build_planned_render;
use nvim_oxi::Result;

pub(crate) fn execute_core_request_render_plan_effect(
    payload: RequestRenderPlanEffect,
) -> Result<Vec<CoreEvent>> {
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
            payload.should_schedule_next_animation,
            payload.next_animation_at_ms,
        )
    }));

    let follow_up = match outcome {
        Ok(planned_render) => CoreEvent::RenderPlanComputed(RenderPlanComputedEvent {
            proposal_id: payload.proposal_id,
            planned_render,
            observed_at: payload.requested_at,
        }),
        Err(_) => {
            warn("core render planning panicked");
            CoreEvent::RenderPlanFailed(RenderPlanFailedEvent {
                proposal_id: payload.proposal_id,
                observed_at: payload.requested_at,
            })
        }
    };

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

    Ok(vec![follow_up])
}
