use super::*;

fn staged_draw_state() -> CoreState {
    planned_state_after_animation_tick(ready_state_with_observation(cursor(9, 9)), 61).0
}

#[test]
fn animation_timer_draw_advances_render_revisions_when_semantics_change() {
    let previous_state = ready_state_with_observation(cursor(9, 9));
    let state = staged_draw_state();
    let scene = state.scene();

    pretty_assert_eq!(scene.semantic_revision().value(), 1);
    pretty_assert_eq!(scene.motion_revision().value(), 1);
    assert_ne!(previous_state.scene().cursor_trail(), scene.cursor_trail());
    assert!(scene.cursor_trail().is_some());
}

#[test]
fn animation_timer_draw_populates_retained_projection_from_the_retained_observation() {
    let state = staged_draw_state();
    let projection = state
        .scene()
        .retained_projection()
        .expect("retained projection after draw render")
        .clone();

    pretty_assert_eq!(projection.witness().observation_id().value(), 9);
    pretty_assert_eq!(
        projection.witness().viewport(),
        ViewportSnapshot::new(CursorRow(40), CursorCol(120))
    );
    pretty_assert_eq!(
        projection
            .cached_logical_raster()
            .clear()
            .map(|clear| clear.max_kept_windows),
        Some(state.runtime().config.max_kept_windows)
    );
}

#[test]
fn animation_timer_draw_stages_a_draw_proposal_against_the_retained_projection_target() {
    let state = staged_draw_state();
    let scene = state.scene();
    let projection = scene
        .retained_projection()
        .expect("retained projection after draw render")
        .clone();
    let Some(proposal) = state.pending_proposal() else {
        panic!("expected staged render proposal");
    };
    let RealizationPlan::Draw(draw) = proposal.realization() else {
        panic!("expected draw realization plan");
    };

    pretty_assert_eq!(
        scene
            .retained_projection()
            .expect("retained projection after draw render")
            .reuse_key()
            .target_cell_presentation(),
        proposal.side_effects().target_cell_presentation
    );
    pretty_assert_eq!(
        draw.palette().color_levels(),
        state.runtime().config.color_levels
    );
    pretty_assert_eq!(
        draw.max_kept_windows(),
        state.runtime().config.max_kept_windows
    );
    pretty_assert_eq!(proposal.patch().basis().target(), Some(&projection));
}
