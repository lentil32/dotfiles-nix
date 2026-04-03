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
    let progress = BackgroundProbeProgress::new(
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
        let progress = snapshot
            .background_progress()
            .expect("requested background probe should own chunk progress");
        let chunk = progress.next_chunk().expect("remaining background chunk");
        let allowed_mask = vec![true; chunk.len()];
        let packed_mask = BackgroundProbeChunkMask::from_allowed_mask(&allowed_mask);

        match progress
            .apply_chunk(&chunk, &packed_mask)
            .expect("chunk should match the active progress cursor")
        {
            BackgroundProbeUpdate::InProgress(next_progress) => {
                saw_in_progress = true;
                snapshot = snapshot
                    .with_background_progress(next_progress)
                    .expect("requested background probe should keep progress");
            }
            BackgroundProbeUpdate::Complete(batch) => {
                snapshot = snapshot
                    .with_background_probe(ProbeState::ready(
                        probe_request_id,
                        request.observation_id(),
                        ProbeReuse::Exact,
                        batch,
                    ))
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
        snapshot.probes().background(),
        ProbeSlot::Requested(ProbeState::Ready { .. })
    ));
    assert!(snapshot.background_probe().is_some());
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
                position: Point { row: 6.1, col: 1.2 },
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
    let progress = BackgroundProbeProgress::new(
        viewport,
        BackgroundProbePlan::from_render_frame(&frame, viewport),
    );
    let chunk = progress
        .next_chunk()
        .expect("probe plan should keep the visible non-target cell");

    assert_eq!(
        chunk.cells(),
        &[ScreenCell::new(1, 3).expect("visible non-target screen cell")]
    );
}
