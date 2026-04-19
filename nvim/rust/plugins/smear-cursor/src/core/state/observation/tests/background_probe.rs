use super::*;
use crate::core::runtime_reducer::build_render_frame;
use crate::core::state::BufferPerfClass;
use crate::state::CursorLocation;
use crate::state::CursorShape;
use crate::state::RuntimeState;
use crate::types::Particle;
use crate::types::Point;
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
fn background_probe_progress_materializes_particles_from_packed_chunk_masks() {
    let viewport = ViewportSnapshot::new(CursorRow(2), CursorCol(5));
    let mut progress = BackgroundProbeProgress::new(
        viewport,
        BackgroundProbePlan::from_cells(vec![
            ScreenCell::new(1, 1).expect("cell"),
            ScreenCell::new(1, 2).expect("cell"),
            ScreenCell::new(1, 3).expect("cell"),
            ScreenCell::new(1, 4).expect("cell"),
            ScreenCell::new(1, 5).expect("cell"),
            ScreenCell::new(2, 1).expect("cell"),
            ScreenCell::new(2, 2).expect("cell"),
            ScreenCell::new(2, 3).expect("cell"),
            ScreenCell::new(2, 4).expect("cell"),
            ScreenCell::new(2, 5).expect("cell"),
        ]),
    );
    let chunk = progress.next_chunk().expect("single chunk viewport");
    let packed_mask =
        BackgroundProbeChunkMask::from_packed_bytes(10, vec![0b0000_0010, 0b0000_0010])
            .expect("packed chunk mask should decode");

    let Some(BackgroundProbeUpdate::Complete(batch)) = progress.apply_chunk(&chunk, &packed_mask)
    else {
        panic!("packed chunk should complete a ten-cell sparse probe");
    };

    assert!(batch.allows_particle(ScreenCell::new(1, 2).expect("allowed cell")));
    assert!(batch.allows_particle(ScreenCell::new(2, 5).expect("allowed cell")));
    assert!(!batch.allows_particle(ScreenCell::new(1, 1).expect("blocked cell")));
}

#[test]
fn requested_background_probe_tracks_progress_until_completion() {
    let request = observation_request(ProbeRequestSet::new(false, true));
    let viewport = ViewportSnapshot::new(CursorRow(600), CursorCol(4));
    let cells = (0_i64..2050_i64)
        .map(|index| {
            let row = index / 4 + 1;
            let col = index % 4 + 1;
            ScreenCell::new(row, col).expect("cell")
        })
        .collect::<Vec<_>>();
    let mut snapshot = ObservationSnapshot::new(
        request.clone(),
        observation_basis(&request, viewport),
        ObservationMotion::default(),
    )
    .with_background_probe_plan(BackgroundProbePlan::from_cells(cells));
    let probe_request_id = ProbeKind::Background.request_id(request.observation_id());
    let mut saw_in_progress = false;

    loop {
        let update = {
            let progress = snapshot
                .background_progress_mut()
                .expect("requested background probe should own chunk progress");
            let chunk = progress.next_chunk().expect("remaining background chunk");
            let allowed_mask = vec![true; chunk.len()];
            let packed_mask = BackgroundProbeChunkMask::from_allowed_mask(&allowed_mask);

            progress
                .apply_chunk(&chunk, &packed_mask)
                .expect("chunk should match the active progress cursor")
        };

        match update {
            BackgroundProbeUpdate::InProgress => {
                saw_in_progress = true;
            }
            BackgroundProbeUpdate::Complete(batch) => {
                snapshot = snapshot
                    .with_background_probe_ready(
                        probe_request_id,
                        request.observation_id(),
                        ProbeReuse::Exact,
                        batch,
                    )
                    .expect("requested background probe should complete");
                break;
            }
        }
    }

    assert!(
        saw_in_progress,
        "viewport should require multiple background chunks"
    );
    assert!(snapshot.background_progress().is_none());
    assert!(matches!(
        snapshot.background_probe_state(),
        BackgroundProbeState::Ready { .. }
    ));
    assert!(snapshot.background_probe().is_some());
}

#[test]
fn requested_background_probe_preserves_sparse_bits_across_chunk_completion() {
    let request = observation_request(ProbeRequestSet::new(false, true));
    let viewport = ViewportSnapshot::new(CursorRow(600), CursorCol(4));
    let cells = (0_i64..2050_i64)
        .map(|index| {
            let row = index / 4 + 1;
            let col = index % 4 + 1;
            ScreenCell::new(row, col).expect("cell")
        })
        .collect::<Vec<_>>();
    let mut snapshot = ObservationSnapshot::new(
        request.clone(),
        observation_basis(&request, viewport),
        ObservationMotion::default(),
    )
    .with_background_probe_plan(BackgroundProbePlan::from_cells(cells));

    let first_chunk = snapshot
        .background_progress()
        .expect("requested background probe should emit a first chunk")
        .next_chunk()
        .expect("first background chunk");
    let mut first_mask = vec![false; first_chunk.len()];
    let last_first_index = first_mask.len().saturating_sub(1);
    first_mask[0] = true;
    first_mask[last_first_index] = true;
    let first_update = {
        let progress = snapshot
            .background_progress_mut()
            .expect("requested background probe should remain pending");
        progress
            .apply_chunk(
                &first_chunk,
                &BackgroundProbeChunkMask::from_allowed_mask(&first_mask),
            )
            .expect("first chunk should advance progress")
    };
    assert_eq!(first_update, BackgroundProbeUpdate::InProgress);

    let second_chunk = snapshot
        .background_progress()
        .expect("requested background probe should emit a second chunk")
        .next_chunk()
        .expect("second background chunk");
    let mut second_mask = vec![false; second_chunk.len()];
    second_mask[0] = true;
    let BackgroundProbeUpdate::Complete(batch) = snapshot
        .background_progress_mut()
        .expect("requested background probe should keep pending progress")
        .apply_chunk(
            &second_chunk,
            &BackgroundProbeChunkMask::from_allowed_mask(&second_mask),
        )
        .expect("second chunk should complete progress")
    else {
        panic!("second chunk should complete a two-chunk sparse probe");
    };

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
    let tracked = CursorLocation::new(10, 20, 1, 1);
    state.initialize_cursor(
        Point { row: 2.0, col: 2.0 },
        CursorShape::new(false, false),
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
                position: Point { row: 2.2, col: 2.4 },
                velocity: Point::ZERO,
                lifetime: 0.75,
            },
            Particle {
                position: Point { row: 1.1, col: 3.4 },
                velocity: Point::ZERO,
                lifetime: 0.75,
            },
            Particle {
                position: Point { row: 1.1, col: 6.2 },
                velocity: Point::ZERO,
                lifetime: 0.75,
            },
            Particle {
                position: Point { row: 2.1, col: 4.2 },
                velocity: Point::ZERO,
                lifetime: 0.75,
            },
        ],
        previous_center: state.previous_center(),
        index_head: 0,
        index_tail: 0,
        rng_state: state.rng_state(),
    });
    let viewport = ViewportSnapshot::new(CursorRow(5), CursorCol(5));
    let current_corners = state.current_corners();
    let target_position = state.target_position();
    let frame = build_render_frame(
        &mut state,
        "n",
        current_corners,
        Vec::new(),
        0,
        target_position,
        false,
        BufferPerfClass::Full,
    );
    let plan = BackgroundProbePlan::from_render_frame(&frame, viewport);
    let progress = BackgroundProbeProgress::new(viewport, plan.clone());
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
fn background_probe_request_id_progress_and_terminal_batch_share_one_state_node() {
    let request = observation_request(ProbeRequestSet::new(false, true));
    let viewport = ViewportSnapshot::new(CursorRow(4), CursorCol(4));
    let probe_request_id = ProbeKind::Background.request_id(request.observation_id());
    let mut snapshot = ObservationSnapshot::new(
        request.clone(),
        observation_basis(&request, viewport),
        ObservationMotion::default(),
    )
    .with_background_probe_plan(BackgroundProbePlan::from_cells(vec![
        ScreenCell::new(1, 1).expect("cell"),
    ]));

    assert_eq!(
        snapshot.background_probe_request_id(),
        Some(probe_request_id)
    );
    assert!(matches!(
        snapshot.background_probe_state(),
        BackgroundProbeState::Collecting { .. }
    ));

    let chunk = snapshot
        .background_progress()
        .and_then(BackgroundProbeProgress::next_chunk)
        .expect("collecting background probe should expose the next chunk");
    assert!(snapshot.apply_background_probe_chunk(
        probe_request_id,
        &chunk,
        &BackgroundProbeChunkMask::from_allowed_mask(&[true]),
    ));

    assert_eq!(
        snapshot.background_probe_request_id(),
        Some(probe_request_id)
    );
    assert!(snapshot.background_progress().is_none());
    assert!(snapshot.background_probe().is_some());
    assert!(matches!(
        snapshot.background_probe_state(),
        BackgroundProbeState::Ready { .. }
    ));
}

#[test]
fn background_probe_chunk_updates_only_apply_while_collecting() {
    let request = observation_request(ProbeRequestSet::new(false, true));
    let viewport = ViewportSnapshot::new(CursorRow(4), CursorCol(4));
    let probe_request_id = ProbeKind::Background.request_id(request.observation_id());
    let plan = BackgroundProbePlan::from_cells(vec![ScreenCell::new(1, 1).expect("cell")]);
    let progress = BackgroundProbeProgress::new(viewport, plan.clone());
    let chunk = progress
        .next_chunk()
        .expect("single-cell background probe plan should emit a chunk");
    let allowed_mask = BackgroundProbeChunkMask::from_allowed_mask(&[true]);

    let mut unrequested_snapshot = ObservationSnapshot::new(
        request.clone(),
        observation_basis(&request, viewport),
        ObservationMotion::default(),
    );
    assert!(!unrequested_snapshot.apply_background_probe_chunk(
        probe_request_id,
        &chunk,
        &allowed_mask,
    ));

    let mut collecting_snapshot = ObservationSnapshot::new(
        request.clone(),
        observation_basis(&request, viewport),
        ObservationMotion::default(),
    )
    .with_background_probe_plan(plan.clone());
    assert!(collecting_snapshot.apply_background_probe_chunk(
        probe_request_id,
        &chunk,
        &allowed_mask,
    ));

    let mut ready_snapshot = ObservationSnapshot::new(
        request.clone(),
        observation_basis(&request, viewport),
        ObservationMotion::default(),
    )
    .with_background_probe_plan(BackgroundProbePlan::from_cells(Vec::new()));
    assert!(!ready_snapshot.apply_background_probe_chunk(probe_request_id, &chunk, &allowed_mask,));

    let mut failed_snapshot = ObservationSnapshot::new(
        request.clone(),
        observation_basis(&request, viewport),
        ObservationMotion::default(),
    )
    .with_background_probe_plan(plan)
    .with_background_probe_failed(probe_request_id, ProbeFailure::ShellReadFailed)
    .expect("collecting background probe should accept a failure transition");
    assert!(
        !failed_snapshot.apply_background_probe_chunk(probe_request_id, &chunk, &allowed_mask,)
    );
}

#[test]
fn background_probe_terminal_states_reject_further_transitions() {
    let request = observation_request(ProbeRequestSet::new(false, true));
    let viewport = ViewportSnapshot::new(CursorRow(4), CursorCol(4));
    let probe_request_id = ProbeKind::Background.request_id(request.observation_id());
    let plan = BackgroundProbePlan::from_cells(vec![ScreenCell::new(1, 1).expect("cell")]);
    let mut ready_snapshot = ObservationSnapshot::new(
        request.clone(),
        observation_basis(&request, viewport),
        ObservationMotion::default(),
    )
    .with_background_probe_plan(plan.clone());
    let chunk = ready_snapshot
        .background_progress()
        .and_then(BackgroundProbeProgress::next_chunk)
        .expect("single-cell background probe plan should emit a chunk");
    assert!(ready_snapshot.apply_background_probe_chunk(
        probe_request_id,
        &chunk,
        &BackgroundProbeChunkMask::from_allowed_mask(&[true]),
    ));
    let ready_batch = ready_snapshot
        .background_probe()
        .cloned()
        .expect("completed background probe should retain the sampled batch");
    assert!(!ready_snapshot.set_background_probe_ready(
        probe_request_id,
        request.observation_id(),
        ProbeReuse::Exact,
        ready_batch,
    ));
    assert!(
        !ready_snapshot
            .set_background_probe_failed(probe_request_id, ProbeFailure::ShellReadFailed)
    );

    let mut failed_snapshot = ObservationSnapshot::new(
        request.clone(),
        observation_basis(&request, viewport),
        ObservationMotion::default(),
    )
    .with_background_probe_plan(plan)
    .with_background_probe_failed(probe_request_id, ProbeFailure::ShellReadFailed)
    .expect("collecting background probe should accept a failure transition");
    assert!(!failed_snapshot.set_background_probe_ready(
        probe_request_id,
        request.observation_id(),
        ProbeReuse::Exact,
        BackgroundProbeBatch::empty(viewport),
    ));
    assert!(
        !failed_snapshot
            .set_background_probe_failed(probe_request_id, ProbeFailure::ShellReadFailed)
    );
}

#[test]
fn empty_background_probe_plan_becomes_ready_without_collecting_progress() {
    let request = observation_request(ProbeRequestSet::new(false, true));
    let viewport = ViewportSnapshot::new(CursorRow(4), CursorCol(4));
    let snapshot = ObservationSnapshot::new(
        request.clone(),
        observation_basis(&request, viewport),
        ObservationMotion::default(),
    )
    .with_background_probe_plan(BackgroundProbePlan::from_cells(Vec::new()));

    assert_eq!(
        snapshot.background_probe_request_id(),
        Some(ProbeKind::Background.request_id(request.observation_id()))
    );
    assert!(snapshot.background_progress().is_none());
    assert!(matches!(
        snapshot.background_probe_state(),
        BackgroundProbeState::Ready { reuse, .. } if *reuse == ProbeReuse::Exact
    ));
    assert!(snapshot.background_probe().is_some());
}
