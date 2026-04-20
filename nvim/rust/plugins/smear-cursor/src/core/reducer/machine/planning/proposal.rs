use super::scene::patch_basis;
use super::scene::realization_plan_for_render_decision;
use super::scene::update_scene_from_render_decision_with_context;
use crate::core::effect::RenderPlanningContext;
use crate::core::runtime_reducer::RenderCleanupAction;
use crate::core::runtime_reducer::RenderDecision;
use crate::core::runtime_reducer::RenderSideEffects;
use crate::core::state::AnimationSchedule;
use crate::core::state::InFlightProposal;
use crate::core::state::PlannedRender;
use crate::core::state::RealizationPlan;
use crate::core::state::ScenePatch;
use crate::core::types::ProposalId;

fn build_in_flight_proposal(
    proposal_id: ProposalId,
    patch: ScenePatch,
    realization: RealizationPlan,
    render_cleanup_action: RenderCleanupAction,
    render_side_effects: RenderSideEffects,
    animation_schedule: AnimationSchedule,
) -> Result<InFlightProposal, crate::core::state::ProposalShapeError> {
    match realization {
        RealizationPlan::Draw(draw) => InFlightProposal::draw(
            proposal_id,
            patch,
            draw,
            render_cleanup_action,
            render_side_effects,
            animation_schedule,
        ),
        RealizationPlan::Clear(clear) => InFlightProposal::clear(
            proposal_id,
            patch,
            clear,
            render_cleanup_action,
            render_side_effects,
            animation_schedule,
        ),
        RealizationPlan::Noop => InFlightProposal::noop(
            proposal_id,
            patch,
            render_cleanup_action,
            render_side_effects,
            animation_schedule,
        ),
        RealizationPlan::Failure(failure) => Ok(InFlightProposal::failure(
            proposal_id,
            patch,
            failure,
            render_cleanup_action,
            render_side_effects,
            animation_schedule,
        )),
    }
}

pub(crate) fn build_planned_render(
    planning: RenderPlanningContext,
    proposal_id: ProposalId,
    render_decision: &RenderDecision,
    animation_schedule: AnimationSchedule,
) -> Result<PlannedRender, crate::core::state::ProposalShapeError> {
    let (current_scene, observation, acknowledged_projection, config) = planning.into_parts();
    let (scene_update, projection, projection_failure) =
        update_scene_from_render_decision_with_context(
            &current_scene,
            observation.as_ref(),
            acknowledged_projection.as_ref(),
            render_decision,
        );
    let realization = realization_plan_for_render_decision(
        config.as_ref(),
        render_decision,
        projection.as_ref(),
        projection_failure,
    );
    let basis = patch_basis(acknowledged_projection, projection);
    let patch = ScenePatch::derive(basis);
    build_in_flight_proposal(
        proposal_id,
        patch,
        realization,
        render_decision.render_cleanup_action,
        render_decision.render_side_effects,
        animation_schedule,
    )
    .map(|proposal| PlannedRender::new(scene_update, proposal))
}

#[cfg(test)]
mod tests {
    use super::build_in_flight_proposal;
    use super::build_planned_render;
    use crate::config::RuntimeConfig;
    use crate::core::effect::RenderPlanningContext;
    use crate::core::realization::PaletteSpec;
    use crate::core::runtime_reducer::RenderAction;
    use crate::core::runtime_reducer::RenderDecision;
    use crate::core::state::AnimationSchedule;
    use crate::core::state::RealizationClear;
    use crate::core::state::RealizationDraw;
    use crate::core::state::RealizationPlan;
    use crate::core::state::ScenePatch;
    use crate::core::state::SceneState;
    use crate::core::types::ProposalId;
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    use super::super::test_support::base_frame;

    #[test]
    fn build_planned_render_uses_captured_config_for_clear_budget() {
        let decision = RenderDecision {
            render_action: RenderAction::ClearAll,
            render_cleanup_action: crate::core::runtime_reducer::RenderCleanupAction::NoAction,
            render_allocation_policy:
                crate::core::runtime_reducer::RenderAllocationPolicy::ReuseOnly,
            render_side_effects: crate::core::runtime_reducer::RenderSideEffects::default(),
        };
        let config = RuntimeConfig {
            max_kept_windows: 13,
            ..RuntimeConfig::default()
        };

        let planned_render = build_planned_render(
            RenderPlanningContext::new(SceneState::default(), None, None, Arc::new(config.clone())),
            ProposalId::new(41),
            &decision,
            AnimationSchedule::Idle,
        )
        .expect("clear planning should preserve proposal shape invariants");

        assert_eq!(
            planned_render.proposal().realization(),
            RealizationPlan::Clear(RealizationClear::new(config.max_kept_windows))
        );
    }

    #[test]
    fn invalid_proposal_shape_returns_typed_error_instead_of_panicking() {
        let patch = ScenePatch::derive(crate::core::state::PatchBasis::new(None, None));

        let result = build_in_flight_proposal(
            ProposalId::new(11),
            patch,
            RealizationPlan::Draw(RealizationDraw::new(
                PaletteSpec::from_frame(&base_frame()),
                crate::core::runtime_reducer::RenderAllocationPolicy::ReuseOnly,
                32,
            )),
            crate::core::runtime_reducer::RenderCleanupAction::NoAction,
            crate::core::runtime_reducer::RenderSideEffects::default(),
            AnimationSchedule::Idle,
        );

        assert_eq!(
            result,
            Err(crate::core::state::ProposalShapeError::DrawMissingTargetProjection)
        );
    }
}
