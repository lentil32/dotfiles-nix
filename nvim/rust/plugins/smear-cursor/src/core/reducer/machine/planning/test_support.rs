use crate::core::runtime_reducer::TargetCellPresentation;
use crate::core::state::BackgroundProbeChunkMask;
use crate::core::state::BackgroundProbePlan;
use crate::core::state::BufferPerfClass;
use crate::core::state::ExternalDemand;
use crate::core::state::ExternalDemandKind;
use crate::core::state::ObservationBasis;
use crate::core::state::ObservationMotion;
use crate::core::state::ObservationSnapshot;
use crate::core::state::PendingObservation;
use crate::core::state::ProbeKind;
use crate::core::state::ProbeRequestSet;
use crate::core::types::IngressSeq;
use crate::core::types::MotionRevision;
use crate::core::types::ProjectionPolicyRevision;
use crate::core::types::RenderRevision;
use crate::core::types::SemanticRevision;
use crate::core::types::StrokeId;
use crate::position::CursorObservation;
use crate::position::ObservedCell;
use crate::position::RenderPoint;
use crate::position::ScreenCell;
use crate::position::ViewportBounds;
use crate::state::TrackedCursor;
use crate::types::CursorCellShape;
use crate::types::ModeClass;
use crate::types::Particle;
use crate::types::RenderFrame;
use crate::types::RenderStepSample;
use crate::types::StaticRenderConfig;
use proptest::prelude::*;
use std::sync::Arc;

pub(super) fn screen_cell(row: i64, col: i64) -> ScreenCell {
    ScreenCell::new(row, col).expect("positive cursor position")
}

fn viewport_bounds(max_row: i64, max_col: i64) -> ViewportBounds {
    ViewportBounds::new(max_row, max_col).expect("positive viewport bounds")
}

pub(super) fn observation_basis(
    seq: u64,
    observed_cell: ObservedCell,
    location: TrackedCursor,
) -> ObservationBasis {
    ObservationBasis::new(
        crate::core::types::Millis::new(seq),
        "n".to_string(),
        location.surface(),
        CursorObservation::new(location.buffer_line(), observed_cell),
        viewport_bounds(40, 120),
    )
}

pub(super) fn valid_surface_location() -> TrackedCursor {
    TrackedCursor::fixture(1, 1, 1, 1)
        .with_window_origin(1, 1)
        .with_window_dimensions(120, 40)
}

pub(super) fn base_frame() -> RenderFrame {
    let corners = [
        RenderPoint {
            row: 12.0,
            col: 10.0,
        },
        RenderPoint {
            row: 12.0,
            col: 11.0,
        },
        RenderPoint {
            row: 13.0,
            col: 11.0,
        },
        RenderPoint {
            row: 13.0,
            col: 10.0,
        },
    ];
    RenderFrame {
        mode: ModeClass::NormalLike,
        corners,
        step_samples: vec![RenderStepSample::new(corners, 1.0)].into(),
        planner_idle_steps: 0,
        target: RenderPoint {
            row: 10.0,
            col: 10.0,
        },
        target_corners: [
            RenderPoint {
                row: 10.0,
                col: 10.0,
            },
            RenderPoint {
                row: 10.0,
                col: 11.0,
            },
            RenderPoint {
                row: 11.0,
                col: 11.0,
            },
            RenderPoint {
                row: 11.0,
                col: 10.0,
            },
        ],
        vertical_bar: false,
        trail_stroke_id: StrokeId::new(1),
        retarget_epoch: 1,
        particle_count: 0,
        aggregated_particle_cells: Arc::default(),
        particle_screen_cells: Arc::default(),
        color_at_cursor: None,
        projection_policy_revision: ProjectionPolicyRevision::INITIAL,
        static_config: Arc::new(StaticRenderConfig {
            cursor_color: None,
            cursor_color_insert_mode: None,
            normal_bg: None,
            transparent_bg_fallback_color: "#303030".to_string(),
            cterm_cursor_colors: None,
            cterm_bg: None,
            hide_target_hack: true,
            max_kept_windows: 32,
            never_draw_over_target: false,
            particle_max_lifetime: 1.0,
            particle_switch_octant_braille: 0.3,
            particles_over_text: true,
            color_levels: 16,
            gamma: 2.2,
            block_aspect_ratio: crate::config::DEFAULT_BLOCK_ASPECT_RATIO,
            tail_duration_ms: 180.0,
            simulation_hz: 120.0,
            trail_thickness: 1.0,
            trail_thickness_x: 1.0,
            spatial_coherence_weight: 1.0,
            temporal_stability_weight: 0.12,
            top_k_per_cell: 5,
            windows_zindex: 200,
        }),
    }
}

pub(super) fn render_revision(
    motion_revision: MotionRevision,
    semantic_revision: SemanticRevision,
) -> RenderRevision {
    RenderRevision::new(motion_revision, semantic_revision)
}

pub(super) fn projection_policy_revision(frame: &RenderFrame) -> ProjectionPolicyRevision {
    frame.projection_policy_revision
}

pub(super) fn observation(seq: u64) -> ObservationSnapshot {
    let request = PendingObservation::new(
        ExternalDemand::new(
            IngressSeq::new(seq),
            ExternalDemandKind::ExternalCursor,
            crate::core::types::Millis::new(seq),
            BufferPerfClass::Full,
        ),
        ProbeRequestSet::default(),
    );
    let basis = observation_basis(
        seq,
        ObservedCell::Exact(screen_cell(10, 10)),
        valid_surface_location(),
    );
    ObservationSnapshot::new(request, basis, ObservationMotion::default())
}

fn observation_with_background_probe(
    seq: u64,
    allowed_cells: &[ScreenCell],
) -> ObservationSnapshot {
    let request = PendingObservation::new(
        ExternalDemand::new(
            IngressSeq::new(seq),
            ExternalDemandKind::ExternalCursor,
            crate::core::types::Millis::new(seq),
            BufferPerfClass::Full,
        ),
        ProbeRequestSet::only(ProbeKind::Background),
    );
    let basis = observation_basis(
        seq,
        ObservedCell::Exact(screen_cell(10, 10)),
        valid_surface_location(),
    );
    let mut snapshot = ObservationSnapshot::new(request, basis, ObservationMotion::default());
    *snapshot.probes_mut().background_mut() = crate::core::state::BackgroundProbeState::from_plan(
        BackgroundProbePlan::from_cells(allowed_cells.to_vec()),
    );
    let chunk = snapshot
        .probes()
        .background()
        .next_chunk()
        .expect("single-cell sparse probe should emit one chunk");
    let allowed_mask = BackgroundProbeChunkMask::from_allowed_mask(&vec![true; chunk.len()]);
    let viewport = snapshot.basis().viewport();
    assert!(
        snapshot
            .probes_mut()
            .background_mut()
            .apply_chunk(viewport, &chunk, &allowed_mask),
        "single chunk sparse probe should complete immediately",
    );
    snapshot
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DirtyMutationAxis {
    None,
    PaletteOnly,
    Presentation,
    Geometry,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ReuseMutationAxis {
    Exact,
    ObservationWitness,
    MotionRevision,
    SemanticRevision,
    ParticleOverlay,
    Presentation,
    Policy,
}

pub(super) fn dirty_mutation_axis_strategy() -> BoxedStrategy<DirtyMutationAxis> {
    prop_oneof![
        Just(DirtyMutationAxis::None),
        Just(DirtyMutationAxis::PaletteOnly),
        Just(DirtyMutationAxis::Presentation),
        Just(DirtyMutationAxis::Geometry),
    ]
    .boxed()
}

pub(super) fn reuse_mutation_axis_strategy() -> BoxedStrategy<ReuseMutationAxis> {
    prop_oneof![
        Just(ReuseMutationAxis::Exact),
        Just(ReuseMutationAxis::ObservationWitness),
        Just(ReuseMutationAxis::MotionRevision),
        Just(ReuseMutationAxis::SemanticRevision),
        Just(ReuseMutationAxis::ParticleOverlay),
        Just(ReuseMutationAxis::Presentation),
        Just(ReuseMutationAxis::Policy),
    ]
    .boxed()
}

pub(super) fn target_cell_presentation_strategy() -> BoxedStrategy<TargetCellPresentation> {
    prop_oneof![
        Just(TargetCellPresentation::None),
        Just(TargetCellPresentation::OverlayCursorCell(
            CursorCellShape::Block
        )),
        Just(TargetCellPresentation::OverlayCursorCell(
            CursorCellShape::VerticalBar,
        )),
        Just(TargetCellPresentation::OverlayCursorCell(
            CursorCellShape::HorizontalBar,
        )),
    ]
    .boxed()
}

pub(super) fn alternate_target_cell_presentation(
    target_cell_presentation: TargetCellPresentation,
) -> TargetCellPresentation {
    match target_cell_presentation {
        TargetCellPresentation::None => {
            TargetCellPresentation::OverlayCursorCell(CursorCellShape::Block)
        }
        TargetCellPresentation::OverlayCursorCell(CursorCellShape::Block) => {
            TargetCellPresentation::None
        }
        TargetCellPresentation::OverlayCursorCell(CursorCellShape::VerticalBar) => {
            TargetCellPresentation::OverlayCursorCell(CursorCellShape::HorizontalBar)
        }
        TargetCellPresentation::OverlayCursorCell(CursorCellShape::HorizontalBar) => {
            TargetCellPresentation::OverlayCursorCell(CursorCellShape::VerticalBar)
        }
    }
}

pub(super) fn frame_with_background_probe_requirement(mut frame: RenderFrame) -> RenderFrame {
    let mut static_config = (*frame.static_config).clone();
    static_config.particles_over_text = false;
    frame.static_config = Arc::new(static_config);
    frame.set_particles(std::sync::Arc::new(vec![Particle {
        position: RenderPoint {
            row: 16.2,
            col: 18.4,
        },
        velocity: RenderPoint::ZERO,
        lifetime: 0.75,
    }]));
    frame
}

pub(super) fn observation_for_projection(
    seq: u64,
    requires_background_probe: bool,
) -> ObservationSnapshot {
    if requires_background_probe {
        observation_with_background_probe(
            seq,
            &[ScreenCell::new(16, 18).expect("particle cell should be visible")],
        )
    } else {
        observation(seq)
    }
}

pub(super) fn frame_with_policy_drift(mut frame: RenderFrame) -> RenderFrame {
    frame.projection_policy_revision = frame.projection_policy_revision.next();
    frame
}

pub(super) fn frame_with_particle_overlay_drift(mut frame: RenderFrame) -> RenderFrame {
    frame.set_particles(Arc::new(vec![Particle {
        position: RenderPoint {
            row: 10.75,
            col: 11.25,
        },
        velocity: RenderPoint::ZERO,
        lifetime: 0.8,
    }]));
    frame
}
