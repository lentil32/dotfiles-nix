use super::*;
use crate::core::runtime_reducer::RenderFrameRequest;
use crate::core::runtime_reducer::build_render_frame;
use crate::core::state::BufferPerfClass;
use crate::core::state::ProbeKind;
use crate::position::RenderPoint;
use crate::state::CursorShape;
use crate::state::RuntimeState;
use crate::state::TrackedCursor;
use crate::types::Particle;
use crate::types::StepOutput;
use pretty_assertions::assert_eq;

#[test]
fn background_probe_chunk_mask_decodes_packed_bytes_and_truncates_padding() {
    let mask = BackgroundProbeChunkMask::from_packed_bytes(10, vec![0b1000_1001, 0b1111_1111])
        .expect("packed mask should decode");

    assert_eq!(mask.len(), 10);
    assert_eq!(mask.packed_len(), 2);
    assert_eq!(
        mask.iter().collect::<Vec<_>>(),
        vec![
            true, false, false, true, false, false, false, true, true, true,
        ]
    );
}

#[test]
fn background_probe_completion_rejects_batches_outside_observation_viewport() {
    let request = observation_request(ProbeRequestSet::only(ProbeKind::Background));
    let viewport = viewport_bounds(1, 1);
    let mut snapshot = ObservationSnapshot::new(
        request,
        observation_basis(viewport),
        ObservationMotion::default(),
    );
    snapshot = with_background_probe_plan(
        snapshot,
        BackgroundProbePlan::from_cells(vec![ScreenCell::new(2, 1).expect("cell")]),
    );
    let chunk = next_background_chunk(&snapshot)
        .expect("collecting background probe should expose the next chunk");

    let viewport = snapshot.basis().viewport();
    assert!(!snapshot.probes_mut().background_mut().apply_chunk(
        viewport,
        &chunk,
        &BackgroundProbeChunkMask::from_allowed_mask(&[true]),
    ));
    assert!(matches!(
        snapshot.probes().background(),
        BackgroundProbeState::Collecting { .. }
    ));
    assert_eq!(
        next_background_chunk(&snapshot)
            .expect("rejected completion should keep the same pending chunk"),
        chunk
    );
    assert!(snapshot.probes().background().batch().is_none());
}

#[test]
fn requested_background_probe_preserves_sparse_bits_across_chunk_completion() {
    let request = observation_request(ProbeRequestSet::only(ProbeKind::Background));
    let viewport = viewport_bounds(600, 4);
    let cells = (0_i64..2050_i64)
        .map(|index| {
            let row = index / 4 + 1;
            let col = index % 4 + 1;
            ScreenCell::new(row, col).expect("cell")
        })
        .collect::<Vec<_>>();
    let mut snapshot = ObservationSnapshot::new(
        request,
        observation_basis(viewport),
        ObservationMotion::default(),
    );
    snapshot = with_background_probe_plan(snapshot, BackgroundProbePlan::from_cells(cells));

    let first_chunk = next_background_chunk(&snapshot).expect("first background chunk");
    let mut first_mask = vec![false; first_chunk.len()];
    let last_first_index = first_mask.len().saturating_sub(1);
    first_mask[0] = true;
    first_mask[last_first_index] = true;
    let viewport = snapshot.basis().viewport();
    assert!(snapshot.probes_mut().background_mut().apply_chunk(
        viewport,
        &first_chunk,
        &BackgroundProbeChunkMask::from_allowed_mask(&first_mask),
    ));

    let second_chunk = next_background_chunk(&snapshot).expect("second background chunk");
    let mut second_mask = vec![false; second_chunk.len()];
    second_mask[0] = true;
    assert!(snapshot.probes_mut().background_mut().apply_chunk(
        viewport,
        &second_chunk,
        &BackgroundProbeChunkMask::from_allowed_mask(&second_mask),
    ));
    let batch = snapshot
        .probes()
        .background()
        .batch()
        .cloned()
        .expect("second chunk should complete a two-chunk sparse probe");

    assert!(batch.allows_particle(first_chunk.iter_cells().next().expect("first chunk cell")));
    assert!(
        batch.allows_particle(
            first_chunk
                .iter_cells()
                .last()
                .expect("last cell of the first chunk")
        )
    );
    assert!(
        batch.allows_particle(
            second_chunk
                .iter_cells()
                .next()
                .expect("first cell of the second chunk")
        )
    );
    assert!(
        !batch.allows_particle(
            second_chunk
                .iter_cells()
                .nth(1)
                .expect("second cell of the second chunk")
        )
    );
}

#[test]
fn background_probe_plan_from_render_frame_filters_target_and_out_of_viewport_cells() {
    let mut state = RuntimeState::default();
    state.config.particles_over_text = false;
    let tracked = TrackedCursor::fixture(10, 20, 1, 1);
    state.initialize_cursor(
        RenderPoint { row: 2.0, col: 2.0 },
        CursorShape::block(),
        7,
        &tracked,
    );
    state.apply_step_output(StepOutput {
        current_corners: state.current_corners(),
        velocity_corners: state.velocity_corners(),
        spring_velocity_corners: state.spring_velocity_corners(),
        trail_elapsed_ms: state.trail_elapsed_ms(),
        particles: vec![
            Particle {
                position: RenderPoint { row: 2.2, col: 2.4 },
                velocity: RenderPoint::ZERO,
                lifetime: 0.75,
            },
            Particle {
                position: RenderPoint { row: 1.1, col: 3.4 },
                velocity: RenderPoint::ZERO,
                lifetime: 0.75,
            },
            Particle {
                position: RenderPoint { row: 1.1, col: 6.2 },
                velocity: RenderPoint::ZERO,
                lifetime: 0.75,
            },
            Particle {
                position: RenderPoint { row: 2.1, col: 4.2 },
                velocity: RenderPoint::ZERO,
                lifetime: 0.75,
            },
        ],
        previous_center: state.previous_center(),
        index_head: 0,
        index_tail: 0,
        rng_state: state.rng_state(),
    });
    let viewport = viewport_bounds(5, 5);
    let current_corners = state.current_corners();
    let target_position = state.target_position();
    let frame = build_render_frame(
        &mut state,
        RenderFrameRequest {
            mode: "n",
            render_corners: current_corners,
            step_samples: Vec::new(),
            planner_idle_steps: 0,
            target: target_position,
            vertical_bar: false,
            buffer_perf_class: BufferPerfClass::Full,
        },
    );
    let plan = BackgroundProbePlan::from_render_frame(&frame, viewport);
    let progress = BackgroundProbeProgress::new(plan.clone());
    let chunk = progress
        .next_chunk()
        .expect("probe plan should keep the visible non-target cells");

    assert_eq!(
        chunk.iter_cells().collect::<Vec<_>>(),
        vec![
            ScreenCell::new(1, 3).expect("visible non-target screen cell"),
            ScreenCell::new(2, 4).expect("second visible non-target screen cell"),
        ]
    );
    assert!(plan.shares_source_with(&frame.particle_screen_cells));
}

#[test]
fn background_probe_terminal_states_reject_further_transitions() {
    let request = observation_request(ProbeRequestSet::only(ProbeKind::Background));
    let viewport = viewport_bounds(4, 4);
    let plan = BackgroundProbePlan::from_cells(vec![ScreenCell::new(1, 1).expect("cell")]);
    let mut ready_snapshot = ObservationSnapshot::new(
        request.clone(),
        observation_basis(viewport),
        ObservationMotion::default(),
    );
    ready_snapshot = with_background_probe_plan(ready_snapshot, plan.clone());
    let chunk = next_background_chunk(&ready_snapshot)
        .expect("single-cell background probe plan should emit a chunk");
    let viewport = ready_snapshot.basis().viewport();
    assert!(ready_snapshot.probes_mut().background_mut().apply_chunk(
        viewport,
        &chunk,
        &BackgroundProbeChunkMask::from_allowed_mask(&[true]),
    ));
    let ready_batch = ready_snapshot
        .probes()
        .background()
        .batch()
        .cloned()
        .expect("completed background probe should retain the sampled batch");
    assert!(!ready_snapshot.probes_mut().background_mut().accept_batch(
        viewport,
        ProbeReuse::Exact,
        ready_batch,
    ));
    assert!(
        !ready_snapshot
            .probes_mut()
            .background_mut()
            .set_failed(ProbeFailure::ShellReadFailed)
    );

    let mut failed_snapshot = ObservationSnapshot::new(
        request,
        observation_basis(viewport),
        ObservationMotion::default(),
    );
    failed_snapshot = with_background_probe_plan(failed_snapshot, plan);
    failed_snapshot = with_background_probe_failed(failed_snapshot, ProbeFailure::ShellReadFailed)
        .expect("collecting background probe should accept a failure transition");
    let viewport = failed_snapshot.basis().viewport();
    assert!(!failed_snapshot.probes_mut().background_mut().accept_batch(
        viewport,
        ProbeReuse::Exact,
        BackgroundProbeBatch::empty(),
    ));
    assert!(
        !failed_snapshot
            .probes_mut()
            .background_mut()
            .set_failed(ProbeFailure::ShellReadFailed)
    );
}

#[test]
fn empty_background_probe_plan_becomes_ready_without_collecting_progress() {
    let request = observation_request(ProbeRequestSet::only(ProbeKind::Background));
    let viewport = viewport_bounds(4, 4);
    let snapshot = ObservationSnapshot::new(
        request,
        observation_basis(viewport),
        ObservationMotion::default(),
    );
    let snapshot =
        with_background_probe_plan(snapshot, BackgroundProbePlan::from_cells(Vec::new()));

    assert!(next_background_chunk(&snapshot).is_none());
    assert!(matches!(
        snapshot.probes().background(),
        BackgroundProbeState::Ready { reuse, .. } if *reuse == ProbeReuse::Exact
    ));
    assert!(snapshot.probes().background().batch().is_some());
}
