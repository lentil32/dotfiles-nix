use super::super::logging::trace_lazy;
use super::super::logging::warn;
use crate::core::effect::RequestRenderPlanEffect;
use crate::core::event::Event as CoreEvent;
use crate::core::event::RenderPlanComputedEvent;
use crate::core::event::RenderPlanFailedEvent;
use crate::core::reducer::build_planned_render;

pub(crate) fn execute_core_request_render_plan_effect(
    payload: RequestRenderPlanEffect,
) -> Vec<CoreEvent> {
    let RequestRenderPlanEffect {
        proposal_id,
        planning,
        render_decision,
        animation_schedule,
        requested_at,
    } = payload;
    let observation_id = planning
        .observation()
        .map(crate::core::effect::RenderPlanningObservation::observation_id);

    trace_lazy(|| {
        format!(
            "render_plan_start proposal_id={} observation_id={} requested_at={}",
            proposal_id.value(),
            observation_id.map_or_else(
                || "none".to_string(),
                |observation_id| observation_id.value().to_string(),
            ),
            requested_at.value(),
        )
    });

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        build_planned_render(planning, proposal_id, &render_decision, animation_schedule)
    }));

    let follow_up = outcome.map_or_else(
        |_| {
            warn("core render planning panicked");
            CoreEvent::RenderPlanFailed(RenderPlanFailedEvent {
                proposal_id,
                observed_at: requested_at,
            })
        },
        |planned_render| match planned_render {
            Ok(planned_render) => CoreEvent::RenderPlanComputed(RenderPlanComputedEvent {
                planned_render: Box::new(planned_render),
                observed_at: requested_at,
            }),
            Err(err) => {
                warn(&format!("core render planning failed: {err}"));
                CoreEvent::RenderPlanFailed(RenderPlanFailedEvent {
                    proposal_id,
                    observed_at: requested_at,
                })
            }
        },
    );

    trace_lazy(|| {
        format!(
            "render_plan_result proposal_id={} result={}",
            proposal_id.value(),
            match &follow_up {
                CoreEvent::RenderPlanComputed(_) => "computed",
                CoreEvent::RenderPlanFailed(_) => "failed",
                _ => "unexpected",
            }
        )
    });

    vec![follow_up]
}
