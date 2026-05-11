use super::*;
use pretty_assertions::assert_eq;

#[test]
fn same_cell_external_retarget_updates_target_position_without_bumping_the_retarget_epoch() {
    let (mut state, kickoff) = animating_runtime_after_kickoff(|state| {
        state.config.delay_event_to_smear = 0.0;
    });
    let baseline_epoch = draw_frame(&kickoff)
        .map(|frame| frame.retarget_epoch)
        .expect("kickoff should draw");

    let retarget = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.4, 12.4, 124.0),
        EventSource::External,
    );
    let frame =
        draw_frame(&retarget).expect("same-cell external retarget should still draw immediately");

    assert!(matches!(render_action(&retarget), RenderAction::Draw(_)));
    assert!(retarget.should_schedule_next_animation());
    assert_eq!(
        render_cleanup_action(&retarget),
        RenderCleanupAction::Invalidate
    );
    assert!(state.is_animating());
    assert_eq!(frame.retarget_epoch, baseline_epoch);
    assert_eq!(state.retarget_epoch(), baseline_epoch);
    assert_eq!(
        frame.target,
        RenderPoint {
            row: 5.4,
            col: 12.4,
        }
    );
    assert_eq!(
        state.target_position(),
        RenderPoint {
            row: 5.4,
            col: 12.4,
        }
    );
}

#[test]
fn changed_cell_external_retarget_draws_immediately_and_bumps_the_retarget_epoch_once() {
    let (mut state, kickoff) = animating_runtime_after_kickoff(|state| {
        state.config.delay_event_to_smear = 0.0;
    });
    let kickoff_frame = draw_frame(&kickoff).expect("kickoff should draw");
    let baseline_epoch = kickoff_frame.retarget_epoch;
    let kickoff_stroke_id = kickoff_frame.trail_stroke_id;

    let retarget = reduce_cursor_event(
        &mut state,
        "n",
        event_at(5.0, 24.0, 124.0),
        EventSource::External,
    );
    let frame =
        draw_frame(&retarget).expect("changed-cell external retarget should draw immediately");

    assert!(matches!(render_action(&retarget), RenderAction::Draw(_)));
    assert!(retarget.should_schedule_next_animation());
    assert_eq!(
        render_cleanup_action(&retarget),
        RenderCleanupAction::Invalidate
    );
    assert!(state.is_animating());
    assert_eq!(frame.trail_stroke_id, kickoff_stroke_id);
    assert_eq!(state.trail_stroke_id(), kickoff_stroke_id);
    assert_eq!(frame.retarget_epoch, baseline_epoch.wrapping_add(1));
    assert_eq!(state.retarget_epoch(), baseline_epoch.wrapping_add(1));
    assert_eq!(
        frame.target,
        RenderPoint {
            row: 5.0,
            col: 24.0,
        }
    );
    assert_eq!(
        state.target_position(),
        RenderPoint {
            row: 5.0,
            col: 24.0,
        }
    );
}

#[test]
fn rapid_far_external_retargets_keep_particle_storage_bounded() {
    let (mut state, _) = animating_runtime_after_kickoff(|state| {
        state.config.delay_event_to_smear = 0.0;
        state.config.particle_max_num = 7;
        state.config.particles_per_second = 10_000.0;
        state.config.particles_per_length = 10_000.0;
        state.config.particle_max_lifetime = 60_000.0;
        state.config.min_distance_emit_particles = 0.0;
    });
    let particle_cap = state.config.particle_max_num;
    let mut max_seen_particles = 0_usize;

    for index in 0_u32..240_u32 {
        let target_row = if index % 2 == 0 { 5_000.0 } else { 1.0 };
        let transition = reduce_cursor_event(
            &mut state,
            "n",
            event_at(target_row, 12.0, 124.0 + f64::from(index) * 2.0),
            EventSource::External,
        );
        let frame_particle_count =
            draw_frame(&transition).map_or(0_usize, |frame| frame.particle_count);
        max_seen_particles = max_seen_particles.max(state.particles().len());
        let _ = state.shared_particle_screen_cells();

        assert_eq!(transition.motion_class, MotionClass::DiscontinuousJump);
        assert!(
            state.particles().len() <= particle_cap,
            "state particle count exceeded cap: count={} cap={particle_cap} index={index}",
            state.particles().len(),
        );
        assert!(
            frame_particle_count <= particle_cap,
            "frame particle count exceeded cap: count={frame_particle_count} cap={particle_cap} index={index}",
        );
    }

    assert_eq!(max_seen_particles, particle_cap);
}
