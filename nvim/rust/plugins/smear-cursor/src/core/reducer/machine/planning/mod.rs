mod projection;
mod proposal;
mod runtime;
mod scene;

#[cfg(test)]
mod test_support;

use crate::core::effect::RenderPlanningObservation;
use crate::core::state::BackgroundProbePlan;
use crate::core::state::CoreState;
use crate::core::state::ObservationSnapshot;
use crate::core::state::PreparedObservationPlan;
use crate::core::types::Millis;
use crate::position::ViewportBounds;

pub(crate) use proposal::build_planned_render;

fn render_planning_observation(observation: &ObservationSnapshot) -> RenderPlanningObservation {
    RenderPlanningObservation::new(
        observation.observation_id(),
        observation.basis().viewport(),
        observation.probes().background().batch().cloned(),
    )
}

pub(super) fn prepare_observation_plan(
    state: CoreState,
    previous_observation: Option<&ObservationSnapshot>,
    observation: &ObservationSnapshot,
    observed_at: Millis,
) -> (CoreState, PreparedObservationPlan) {
    runtime::prepare_observation_plan(state, previous_observation, observation, observed_at)
}

pub(super) fn background_probe_plan(
    prepared_plan: &PreparedObservationPlan,
    viewport: ViewportBounds,
) -> Option<BackgroundProbePlan> {
    runtime::background_probe_plan(prepared_plan, viewport)
}

pub(super) fn plan_ready_state(
    state: CoreState,
    previous_observation: Option<&ObservationSnapshot>,
    observed_at: Millis,
) -> super::Transition {
    runtime::plan_ready_state(state, previous_observation, observed_at)
}

pub(super) fn plan_ready_state_with_observation_plan(
    state: CoreState,
    observed_at: Millis,
    prepared_plan: PreparedObservationPlan,
) -> super::Transition {
    runtime::plan_ready_state_with_observation_plan(state, observed_at, prepared_plan)
}
