use super::*;
use crate::core::types::StepIndex;
use crate::core::types::StrokeId;
use crate::types::BASE_TIME_INTERVAL;
use crate::types::Point;
use crate::types::RenderFrame;
use crate::types::RenderStepSample;
use crate::types::StaticRenderConfig;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::Arc;

fn base_frame() -> RenderFrame {
    let corners = [
        Point {
            row: 10.0,
            col: 10.0,
        },
        Point {
            row: 10.0,
            col: 11.0,
        },
        Point {
            row: 11.0,
            col: 11.0,
        },
        Point {
            row: 11.0,
            col: 10.0,
        },
    ];
    RenderFrame {
        mode: "n".to_string(),
        corners,
        step_samples: vec![sample_for_corners(corners)].into(),
        planner_idle_steps: 0,
        target: Point {
            row: 10.0,
            col: 10.0,
        },
        target_corners: corners,
        vertical_bar: false,
        trail_stroke_id: StrokeId::new(1),
        retarget_epoch: 1,
        particles: Vec::new().into(),
        color_at_cursor: None,
        static_config: Arc::new(StaticRenderConfig {
            cursor_color: None,
            cursor_color_insert_mode: None,
            normal_bg: None,
            transparent_bg_fallback_color: "#303030".to_string(),
            cterm_cursor_colors: None,
            cterm_bg: None,
            hide_target_hack: false,
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

fn sample_for_corners(corners: [Point; 4]) -> RenderStepSample {
    RenderStepSample::new(corners, BASE_TIME_INTERVAL)
}

fn unit_square_corners_at(row: i16, col: i16) -> [Point; 4] {
    let row = f64::from(row);
    let col = f64::from(col);
    [
        Point { row, col },
        Point {
            row,
            col: col + 1.0,
        },
        Point {
            row: row + 1.0,
            col: col + 1.0,
        },
        Point {
            row: row + 1.0,
            col,
        },
    ]
}

fn single_sample_frame(row: i16, col: i16) -> RenderFrame {
    let corners = unit_square_corners_at(row, col);
    let mut frame = base_frame();
    frame.corners = corners;
    frame.target_corners = corners;
    frame.step_samples = vec![sample_for_corners(corners)].into();
    frame.target = Point {
        row: f64::from(row),
        col: f64::from(col),
    };
    frame
}

fn quiescent_frame(row: i16, col: i16) -> RenderFrame {
    let mut frame = single_sample_frame(row, col);
    frame.step_samples = Vec::new().into();
    frame
}

fn frames_from_origins(origins: &[(i16, i16)]) -> Vec<RenderFrame> {
    origins
        .iter()
        .map(|&(row, col)| single_sample_frame(row, col))
        .collect()
}

fn set_frame_corners(frame: &mut RenderFrame, corners: [Point; 4]) {
    frame.corners = corners;
    frame.target_corners = corners;
    frame.step_samples = vec![sample_for_corners(corners)].into();
}

fn with_block_aspect_ratio(frame: &RenderFrame, block_aspect_ratio: f64) -> RenderFrame {
    let mut updated = frame.clone();
    let mut static_config = (*updated.static_config).clone();
    static_config.block_aspect_ratio = block_aspect_ratio;
    updated.static_config = Arc::new(static_config);
    updated
}

fn with_trail_thickness(frame: &RenderFrame, trail_thickness: f64) -> RenderFrame {
    let mut updated = frame.clone();
    let mut static_config = (*updated.static_config).clone();
    static_config.trail_thickness = trail_thickness;
    static_config.trail_thickness_x = trail_thickness;
    updated.static_config = Arc::new(static_config);
    updated
}

fn op_cells(output: &super::PlannerOutput) -> BTreeSet<(i64, i64)> {
    output
        .plan
        .cell_ops
        .iter()
        .map(|op| (op.row, op.col))
        .collect::<BTreeSet<_>>()
}

fn highlight_state(level: u32) -> DecodedCellState {
    DecodedCellState {
        glyph: DecodedGlyph::Block,
        level: HighlightLevel::from_raw_clamped(level),
    }
}

fn decoded_state(glyph: DecodedGlyph, level: u32) -> DecodedCellState {
    DecodedCellState {
        glyph,
        level: HighlightLevel::from_raw_clamped(level),
    }
}

fn ordered_candidates(mut candidates: Vec<CellCandidate>) -> Vec<CellCandidate> {
    candidates.sort_by(|lhs, rhs| candidate_cmp(*lhs, *rhs, None));
    candidates
}

fn build_slice_states_reference(slice: &RibbonSlice, spatial_weight_q10: u32) -> Vec<SliceState> {
    let baseline = slice
        .cells
        .iter()
        .fold(0_u64, |acc, cell| acc.saturating_add(cell.empty_cost));
    let peak_highlight_level = slice_peak_highlight_level(slice);
    let empty_state = SliceState::empty(0);
    let mut states = vec![SliceState::empty(
        baseline.saturating_add(state_local_prior(slice, empty_state, spatial_weight_q10)),
    )];

    for start in 0..slice.cells.len() {
        let max_end = (start + RIBBON_MAX_RUN_LENGTH).min(slice.cells.len());
        for end_exclusive in (start + 1)..=max_end {
            let end = end_exclusive - 1;
            let Some(run) = RunSpan::try_new(start, end) else {
                continue;
            };
            let enumeration_input = RunEnumerationInput {
                slice,
                run,
                spatial_weight_q10,
                peak_highlight_level,
            };
            enumerate_run_candidate_states_reference(
                enumeration_input,
                RunEnumerationCursor {
                    cell_index: start,
                    running_cost: baseline,
                },
                &mut [0; RIBBON_MAX_RUN_LENGTH],
                &mut states,
            );
        }
    }

    states.sort_by(|lhs, rhs| slice_state_cmp(*lhs, *rhs));
    states.truncate(RIBBON_MAX_STATES_PER_SLICE);
    states
}

fn enumerate_run_candidate_states_reference(
    input: RunEnumerationInput<'_>,
    cursor: RunEnumerationCursor,
    candidate_offsets: &mut [u8; RIBBON_MAX_RUN_LENGTH],
    states: &mut Vec<SliceState>,
) {
    if cursor.cell_index > input.run.end {
        let state = SliceState::with_run(input.run, *candidate_offsets, 0);
        states.push(SliceState::with_run(
            input.run,
            *candidate_offsets,
            cursor.running_cost.saturating_add(state_local_prior(
                input.slice,
                state,
                input.spatial_weight_q10,
            )),
        ));
        return;
    }

    let offset = cursor.cell_index - input.run.start;
    let cell = &input.slice.cells[cursor.cell_index];
    if cell.non_empty_candidates.is_empty() {
        return;
    }

    for (candidate_index, candidate) in cell.non_empty_candidates.iter().copied().enumerate() {
        let Ok(candidate_index) = u8::try_from(candidate_index) else {
            continue;
        };
        candidate_offsets[offset] = candidate_index;
        let next_cost = cursor
            .running_cost
            .saturating_sub(cell.empty_cost)
            .saturating_add(adjusted_candidate_cost(
                input.slice,
                input.peak_highlight_level,
                candidate,
            ));
        enumerate_run_candidate_states_reference(
            input,
            RunEnumerationCursor {
                cell_index: cursor.cell_index + 1,
                running_cost: next_cost,
            },
            candidate_offsets,
            states,
        );
    }
}

fn slice_cell_with_candidates(
    coord: (i64, i64),
    normal: f64,
    half_span: f64,
    empty_cost: u64,
    non_empty_candidates: Vec<CellCandidate>,
) -> SliceCell {
    SliceCell::new(
        coord,
        to_q16(normal),
        to_q16(half_span),
        empty_cost,
        non_empty_candidates,
    )
}

fn slice_cell(
    coord: (i64, i64),
    normal: f64,
    half_span: f64,
    empty_cost: u64,
    non_empty_level: u32,
    unary_cost: u64,
) -> SliceCell {
    let normal_q16 = to_q16(normal);
    SliceCell::new(
        coord,
        normal_q16,
        to_q16(half_span),
        empty_cost,
        vec![CellCandidate {
            state: Some(highlight_state(non_empty_level)),
            unary_cost,
        }],
    )
}

fn tile_for_octant(mask: u8, sample_q12: u16) -> latent_field::MicroTile {
    let mut tile = latent_field::MicroTile::default();
    for sample_row in 0..latent_field::MICRO_H {
        for sample_col in 0..latent_field::MICRO_W {
            let row_bucket = (sample_row * 4) / latent_field::MICRO_H;
            let col_bucket = (sample_col * 2) / latent_field::MICRO_W;
            let bit = OCTANT_BIT_WEIGHTS[row_bucket][col_bucket];
            if mask & bit == 0 {
                continue;
            }
            let index = sample_row * latent_field::MICRO_W + sample_col;
            tile.samples_q12[index] = sample_q12;
        }
    }
    tile
}

fn tile_for_column_span(
    min_col: usize,
    max_col: usize,
    sample_q12: u16,
) -> latent_field::MicroTile {
    let mut tile = latent_field::MicroTile::default();
    for sample_row in 0..latent_field::MICRO_H {
        for sample_col in min_col..=max_col {
            let index = sample_row * latent_field::MICRO_W + sample_col;
            tile.samples_q12[index] = sample_q12;
        }
    }
    tile
}

fn compiled_single_cell(
    tile: latent_field::MicroTile,
    age: AgeMoment,
) -> BTreeMap<(i64, i64), latent_field::CompiledCell> {
    BTreeMap::from([((10_i64, 10_i64), latent_field::CompiledCell { tile, age })])
}

fn compiled_field(compiled: &BTreeMap<(i64, i64), latent_field::CompiledCell>) -> CompiledField {
    CompiledField::Reference(compiled.clone())
}

fn compiled_rows(
    compiled: &BTreeMap<(i64, i64), latent_field::CompiledCell>,
) -> latent_field::CellRows<latent_field::CompiledCell> {
    let mut rows = latent_field::CellRows::default();
    for (&coord, &cell) in compiled {
        let _ = rows.insert(coord, cell);
    }
    rows
}

fn count_state_toggles(states: &[DecodedCellState]) -> usize {
    states.windows(2).filter(|pair| pair[0] != pair[1]).count()
}

fn dense_slice_with_candidate_fanout(cell_count: usize, candidate_count: usize) -> RibbonSlice {
    let cells = (0..cell_count)
        .map(|cell_index| {
            let candidates = ordered_candidates(
                (0..candidate_count)
                    .map(|candidate_index| CellCandidate {
                        state: Some(decoded_state(
                            DecodedGlyph::Block,
                            2 + ((cell_index * candidate_count + candidate_index) % 13) as u32,
                        )),
                        unary_cost: 40
                            + ((cell_index * 29 + candidate_index * 17 + candidate_index) % 113)
                                as u64,
                    })
                    .collect::<Vec<_>>(),
            );
            slice_cell_with_candidates(
                (10, 10 + cell_index as i64),
                cell_index as f64 * 0.7,
                0.45,
                180 + (cell_index as u64 * 9),
                candidates,
            )
        })
        .collect::<Vec<_>>();

    RibbonSlice {
        cells,
        tail_u: 0.58,
        target_width_cells: 2.4,
        tip_width_cap_cells: COMET_MIN_RESOLVABLE_WIDTH,
        transverse_width_penalty: 0.35,
    }
}

fn render_frame_to_plan_reference(
    frame: &RenderFrame,
    state: PlannerState,
    viewport: Viewport,
) -> PlannerOutput {
    let compiled_frame = compile_render_frame(frame, state.clone());
    let reference_compiled = compile_render_frame_reference(frame, state);

    decode_compiled_frame(
        frame,
        CompiledPlannerFrame {
            next_state: compiled_frame.next_state,
            compiled: Arc::new(CompiledField::Reference(reference_compiled)),
            query_bounds: None,
        },
        viewport,
        frame_draw_signature(frame),
    )
}

fn query_envelope_area_cells(bounds: SliceSearchBounds) -> u64 {
    let row_span = u64::try_from(i128::from(bounds.max_row) - i128::from(bounds.min_row) + 1)
        .unwrap_or(u64::MAX);
    let col_span = u64::try_from(i128::from(bounds.max_col) - i128::from(bounds.min_col) + 1)
        .unwrap_or(u64::MAX);
    row_span.saturating_mul(col_span)
}

fn test_viewport() -> Viewport {
    Viewport {
        max_row: 200,
        max_col: 200,
    }
}

mod cursor_punch_through_and_trail_strokes;
mod decode_path_selection;
mod decode_salience;
mod decode_solver_reuse_and_fallback;
mod draw_signatures_and_determinism;
mod field_compilation_cache;
mod field_reference_and_scratch;
mod projected_span_geometry;
mod ribbon_dp_and_slice_candidates;
mod ribbon_width_targets_and_taper;
mod slice_state_enumeration;
mod staged_deposits_and_metric_projection;
