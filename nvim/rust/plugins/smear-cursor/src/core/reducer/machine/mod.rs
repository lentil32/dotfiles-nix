mod apply;
mod observation;
mod planning;
mod support;
mod timers;

use super::transition::Transition;
use crate::core::event::Event;
use crate::core::state::CoreState;
use apply::reduce_apply_reported;
use apply::reduce_effect_failed;
use apply::reduce_render_cleanup_applied;
use apply::reduce_render_cleanup_retained_resources_observed;
use apply::reduce_render_plan_computed;
use apply::reduce_render_plan_failed;
use observation::reduce_external_demand_queued;
use observation::reduce_initialize;
use observation::reduce_observation_base_collected;
use observation::reduce_probe_reported;
pub(crate) use planning::build_planned_render;
use timers::reduce_timer_signal_with_token;

pub(crate) fn reduce_owned(state: CoreState, event: Event) -> Transition {
    match event {
        Event::Initialize(payload) => reduce_initialize(state, payload),
        Event::ExternalDemandQueued(payload) => reduce_external_demand_queued(state, payload),
        Event::ObservationBaseCollected(payload) => {
            reduce_observation_base_collected(state, payload)
        }
        Event::ProbeReported(payload) => reduce_probe_reported(state, payload),
        Event::RenderPlanComputed(payload) => reduce_render_plan_computed(state, payload),
        Event::RenderPlanFailed(payload) => reduce_render_plan_failed(state, payload),
        Event::ApplyReported(payload) => reduce_apply_reported(state, payload),
        Event::RenderCleanupApplied(payload) => reduce_render_cleanup_applied(state, payload),
        Event::RenderCleanupRetainedResourcesObserved(payload) => {
            reduce_render_cleanup_retained_resources_observed(state, payload)
        }
        Event::TimerFiredWithToken(payload) => {
            reduce_timer_signal_with_token(state, payload.token, payload.observed_at)
        }
        Event::TimerLostWithToken(payload) => {
            reduce_timer_signal_with_token(state, payload.token, payload.observed_at)
        }
        Event::EffectFailed(payload) => reduce_effect_failed(state, payload),
    }
}
