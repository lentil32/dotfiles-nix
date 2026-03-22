use super::*;
use crate::core::types::{StepIndex, StrokeId};
use crate::types::{BASE_TIME_INTERVAL, Point, RenderFrame, RenderStepSample, StaticRenderConfig};
use std::collections::{BTreeMap, BTreeSet};
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

mod field_compilation_and_cache {
    use super::*;

    #[test]
    fn empty_previous_only_cell_keeps_empty_candidate_only() {
        let previous_cells = BTreeMap::from([((10_i64, 10_i64), highlight_state(4))]);
        let candidates = build_cell_candidates(&BTreeMap::new(), &previous_cells, 16, 1.0, 5);

        let cell_candidates = candidates
            .get(&(10_i64, 10_i64))
            .expect("previous cell should stay addressable");
        assert_eq!(cell_candidates.len(), 1);
        assert_eq!(cell_candidates[0].state, None);
    }

    #[test]
    fn visible_cell_with_top_k_one_still_keeps_only_empty_candidate() {
        let compiled = compiled_single_cell(
            tile_for_column_span(0, latent_field::MICRO_W - 1, 0x0FFF),
            AgeMoment::default(),
        );
        let candidates = build_cell_candidates(&compiled, &BTreeMap::new(), 16, 0.0, 1);

        let cell_candidates = candidates
            .get(&(10_i64, 10_i64))
            .expect("compiled cell should stay addressable");
        assert_eq!(cell_candidates.len(), 1);
        assert_eq!(cell_candidates[0].state, None);
    }

    #[test]
    fn overlapping_previous_shade_index_does_not_duplicate_non_empty_candidates() {
        let sample_q12 = quantized_level_to_sample_q12(HighlightLevel::from_raw_clamped(8), 16);
        let compiled = compiled_single_cell(
            tile_for_column_span(0, latent_field::MICRO_W - 1, sample_q12),
            AgeMoment::default(),
        );
        let previous_cells = BTreeMap::from([((10_i64, 10_i64), highlight_state(8))]);
        let candidates = build_cell_candidates(&compiled, &previous_cells, 16, 0.12, 5);

        let cell_candidates = candidates
            .get(&(10_i64, 10_i64))
            .expect("compiled cell should stay addressable");
        let non_empty_states = cell_candidates
            .iter()
            .filter_map(|candidate| candidate.state)
            .collect::<Vec<_>>();

        assert_eq!(
            non_empty_states.len(),
            non_empty_states
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len()
        );
    }

    #[test]
    fn compile_render_frame_reuses_cached_field_when_history_is_unchanged() {
        let mut frame = base_frame();
        frame.step_samples = Vec::new().into();

        let first = compile_render_frame(&frame, PlannerState::default());
        let second = compile_render_frame(&frame, first.next_state.clone());

        assert!(Arc::ptr_eq(&first.compiled, &second.compiled));
    }

    #[test]
    fn compile_render_frame_invalidates_cached_field_after_history_changes() {
        let first = compile_render_frame(&base_frame(), PlannerState::default());
        let mut second_frame = base_frame();
        set_frame_corners(
            &mut second_frame,
            [
                Point {
                    row: 10.0,
                    col: 11.0,
                },
                Point {
                    row: 10.0,
                    col: 12.0,
                },
                Point {
                    row: 11.0,
                    col: 12.0,
                },
                Point {
                    row: 11.0,
                    col: 11.0,
                },
            ],
        );

        let second = compile_render_frame(&second_frame, first.next_state.clone());

        assert!(!Arc::ptr_eq(&first.compiled, &second.compiled));
    }

    #[test]
    fn planner_idle_steps_age_history_without_new_motion_samples() {
        let viewport = Viewport {
            max_row: 200,
            max_col: 200,
        };
        let first = render_frame_to_plan(&base_frame(), PlannerState::default(), viewport);
        let mut draining = base_frame();
        draining.step_samples = Vec::new().into();
        draining.planner_idle_steps = u32::try_from(latent_field::max_comet_support_steps(
            draining.tail_duration_ms,
            draining.simulation_hz,
        ))
        .unwrap_or(u32::MAX);

        let drained = render_frame_to_plan(&draining, first.next_state, viewport);

        assert!(drained.plan.cell_ops.is_empty());
    }
}

mod draw_signatures_and_determinism {
    use super::*;

    #[test]
    fn render_plan_is_deterministic_for_identical_frame_sequence() {
        let first = base_frame();
        let mut second = first.clone();
        set_frame_corners(
            &mut second,
            [
                Point {
                    row: 10.0,
                    col: 12.0,
                },
                Point {
                    row: 10.0,
                    col: 13.0,
                },
                Point {
                    row: 11.0,
                    col: 13.0,
                },
                Point {
                    row: 11.0,
                    col: 12.0,
                },
            ],
        );
        second.target = Point {
            row: 10.5,
            col: 12.5,
        };

        let mut third = second.clone();
        set_frame_corners(
            &mut third,
            [
                Point {
                    row: 10.0,
                    col: 16.0,
                },
                Point {
                    row: 10.0,
                    col: 17.0,
                },
                Point {
                    row: 11.0,
                    col: 17.0,
                },
                Point {
                    row: 11.0,
                    col: 16.0,
                },
            ],
        );
        third.target = Point {
            row: 10.5,
            col: 16.5,
        };

        let frames = [first, second, third];
        let viewport = Viewport {
            max_row: 200,
            max_col: 200,
        };

        let run = |initial: PlannerState| {
            let mut state = initial;
            let mut outputs = Vec::new();
            for frame in &frames {
                let output = render_frame_to_plan(frame, state, viewport);
                state = output.next_state.clone();
                outputs.push((output.plan, output.signature, output.next_state));
            }
            outputs
        };

        let lhs = run(PlannerState::default());
        let rhs = run(PlannerState::default());

        assert_eq!(lhs.len(), rhs.len());
        for index in 0..lhs.len() {
            assert_eq!(lhs[index], rhs[index]);
        }
    }

    #[test]
    fn draw_signature_changes_when_field_config_changes() {
        let first = base_frame();
        let mut second = first.clone();
        let mut static_config = (*second.static_config).clone();
        static_config.tail_duration_ms = 240.0;
        second.static_config = Arc::new(static_config);

        assert_ne!(frame_draw_signature(&first), frame_draw_signature(&second));
    }

    #[test]
    fn draw_signature_changes_when_step_sample_path_changes() {
        let first = base_frame();
        let mut second = first.clone();
        second.step_samples = vec![
            sample_for_corners([
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
            ]),
            sample_for_corners([
                Point {
                    row: 10.0,
                    col: 10.5,
                },
                Point {
                    row: 10.0,
                    col: 11.5,
                },
                Point {
                    row: 11.0,
                    col: 11.5,
                },
                Point {
                    row: 11.0,
                    col: 10.5,
                },
            ]),
        ]
        .into();

        assert_ne!(frame_draw_signature(&first), frame_draw_signature(&second));
    }
}

mod cursor_punch_through_and_trail_strokes {
    use super::*;

    #[test]
    fn render_plan_leaves_target_cell_available_for_cursor_punch_through() {
        let frame = base_frame();
        let viewport = Viewport {
            max_row: 200,
            max_col: 200,
        };

        let output = render_frame_to_plan(&frame, PlannerState::default(), viewport);
        let target = (
            frame.target.row.round() as i64,
            frame.target.col.round() as i64,
        );

        assert!(
            output
                .plan
                .cell_ops
                .iter()
                .all(|op| (op.row, op.col) != target),
            "target cell should remain available for cursor punch-through"
        );
    }

    #[test]
    fn trail_stroke_change_preserves_old_tail_without_bridging() {
        let viewport = Viewport {
            max_row: 200,
            max_col: 200,
        };

        let mut first = base_frame();
        first.target.col = 8.0;
        set_frame_corners(
            &mut first,
            [
                Point {
                    row: 10.0,
                    col: 8.0,
                },
                Point {
                    row: 10.0,
                    col: 9.0,
                },
                Point {
                    row: 11.0,
                    col: 9.0,
                },
                Point {
                    row: 11.0,
                    col: 8.0,
                },
            ],
        );

        let mut second = first.clone();
        second.target.col = 24.0;
        set_frame_corners(
            &mut second,
            [
                Point {
                    row: 10.0,
                    col: 24.0,
                },
                Point {
                    row: 10.0,
                    col: 25.0,
                },
                Point {
                    row: 11.0,
                    col: 25.0,
                },
                Point {
                    row: 11.0,
                    col: 24.0,
                },
            ],
        );

        let after_first =
            render_frame_to_plan(&first, PlannerState::default(), viewport).next_state;

        let mut bridged = second.clone();
        bridged.trail_stroke_id = first.trail_stroke_id;
        let bridged_output = render_frame_to_plan(&bridged, after_first.clone(), viewport);

        let mut reset = second;
        reset.trail_stroke_id = StrokeId::new(first.trail_stroke_id.value().wrapping_add(1));
        let reset_output = render_frame_to_plan(&reset, after_first, viewport);

        let bridged_cells = op_cells(&bridged_output);
        let reset_cells = op_cells(&reset_output);

        assert_eq!(bridged_output.next_state.step_index.value(), 2);
        assert_eq!(reset_output.next_state.step_index.value(), 2);
        assert!(
            bridged_cells.len() >= reset_cells.len(),
            "stroke reset should avoid carrying broad bridge coverage"
        );
        assert!(
            reset_cells
                .iter()
                .all(|(row, col)| !((10..=11).contains(row) && (12..=20).contains(col))),
            "stroke reset should not draw an impossible bridge through the interior gap"
        );
        assert!(
            reset_output.next_state.history.len() >= 2,
            "disconnect should keep prior deposited slices alive so the old tail can fade"
        );
    }
}

mod ribbon_dp_and_slice_candidates {
    use super::*;

    fn transition_cost_linear_baseline(
        previous_slice: &RibbonSlice,
        previous_state: SliceState,
        next_slice: &RibbonSlice,
        next_state: SliceState,
        spatial_weight_q10: u32,
    ) -> u64 {
        let mut cost = 0_u64;
        for (next_index, next_cell) in next_slice.cells.iter().enumerate() {
            let next_value = state_for_slice_cell(next_slice, next_state, next_index);
            if let Some(previous_index) = previous_slice
                .cells
                .iter()
                .position(|previous_cell| previous_cell.coord == next_cell.coord)
            {
                let previous_value =
                    state_for_slice_cell(previous_slice, previous_state, previous_index);
                cost = cost.saturating_add(scale_penalty(
                    overlap_penalty(previous_value, next_value),
                    spatial_weight_q10,
                ));
            }
        }

        let prev_width = run_width_cells(previous_slice, previous_state);
        let next_width = run_width_cells(next_slice, next_state);
        cost = cost.saturating_add(scale_penalty(
            linear_cells_penalty((prev_width - next_width).abs(), PENALTY_THICKNESS_DELTA),
            spatial_weight_q10,
        ));
        let (headward_len, tailward_len) = if previous_slice.tail_u <= next_slice.tail_u {
            (prev_width, next_width)
        } else {
            (next_width, prev_width)
        };
        let mono_violation = (tailward_len - headward_len - COMET_MONO_EPSILON_CELLS).max(0.0);
        cost = cost.saturating_add(scale_penalty(
            squared_cells_penalty(mono_violation, COMET_MONO_WEIGHT),
            spatial_weight_q10,
        ));

        match (
            previous_slice
                .run_projected_span_q16(previous_state)
                .map(ProjectedSpanQ16::center_q16),
            next_slice
                .run_projected_span_q16(next_state)
                .map(ProjectedSpanQ16::center_q16),
        ) {
            (Some(prev_center), Some(next_center)) => {
                let shift_q16 = prev_center.abs_diff(next_center);
                let shift_penalty =
                    (u64::from(shift_q16).saturating_mul(PENALTY_CENTER_SHIFT)) / Q16_SCALE_U64;
                cost = cost.saturating_add(scale_penalty(shift_penalty, spatial_weight_q10));
                if shift_q16 > ((3 * Q16_SCALE) / 2) as u32 {
                    cost =
                        cost.saturating_add(scale_penalty(PENALTY_DISCONNECT, spatial_weight_q10));
                }
            }
            (None, Some(_)) | (Some(_), None) => {
                cost = cost
                    .saturating_add(scale_penalty(PENALTY_EMPTY_TRANSITION, spatial_weight_q10));
            }
            (None, None) => {}
        }

        cost
    }

    #[test]
    fn nearest_shade_lookup_matches_linear_baseline() {
        for color_levels in 1_u32..=64_u32 {
            let shades = build_shade_profiles(color_levels);
            for alpha_q12 in 0_u16..=4095_u16 {
                let expected = shades
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, shade)| shade.sample_q12.abs_diff(alpha_q12))
                    .map(|(index, _)| index);
                assert_eq!(
                    nearest_shade_profile_index(&shades, alpha_q12),
                    expected,
                    "nearest shade mismatch for color_levels={color_levels} alpha_q12={alpha_q12}"
                );
            }
        }
    }

    #[test]
    fn shade_profile_neighbors_reduce_frame_to_frame_toggling() {
        let age = AgeMoment {
            total_mass_q12: 4095,
            recent_mass_q12: 0,
        };
        let boundary_trace = [2174_u16, 2176_u16, 2174_u16, 2176_u16];

        let decode_trace = |temporal_stability_weight: f64| {
            let mut previous_cells = BTreeMap::<(i64, i64), DecodedCellState>::new();
            let mut decoded_states = Vec::<DecodedCellState>::new();

            for sample_q12 in boundary_trace {
                let compiled = compiled_single_cell(tile_for_octant(1, sample_q12), age);
                let candidates = build_cell_candidates(
                    &compiled,
                    &previous_cells,
                    16,
                    temporal_stability_weight,
                    5,
                );
                let decoded = decode_locally(&candidates);
                let state = decoded
                    .get(&(10_i64, 10_i64))
                    .copied()
                    .expect("single-cell trace should decode");
                decoded_states.push(state);
                previous_cells = decoded;
            }

            decoded_states
        };

        let unstable = decode_trace(0.0);
        let stable = decode_trace(3.0);

        assert!(
            count_state_toggles(&stable) < count_state_toggles(&unstable),
            "neighbor shade profiles should let temporal stability suppress boundary flicker: unstable={unstable:?} stable={stable:?}"
        );
    }

    #[test]
    fn ribbon_dp_prefers_consistent_cross_section_over_local_flip() {
        let slice0 = RibbonSlice {
            cells: vec![
                slice_cell((10, 10), -1.0, 0.5, 100, 12, 0),
                slice_cell((10, 11), 0.0, 0.5, 100, 6, 500),
                slice_cell((10, 12), 1.0, 0.5, 100, 10, 20),
            ],
            tail_u: 1.0,
            target_width_cells: 1.0,
            tip_width_cap_cells: COMET_MIN_RESOLVABLE_WIDTH,
            transverse_width_penalty: 0.0,
        };

        let slice1 = RibbonSlice {
            cells: vec![
                slice_cell((11, 10), -1.0, 0.5, 100, 12, 20),
                slice_cell((11, 11), 0.0, 0.5, 100, 6, 500),
                slice_cell((11, 12), 1.0, 0.5, 100, 10, 0),
            ],
            tail_u: 0.5,
            target_width_cells: 1.0,
            tip_width_cap_cells: COMET_MIN_RESOLVABLE_WIDTH,
            transverse_width_penalty: 0.0,
        };

        let slice2 = RibbonSlice {
            cells: vec![
                slice_cell((12, 10), -1.0, 0.5, 100, 12, 0),
                slice_cell((12, 11), 0.0, 0.5, 100, 6, 500),
                slice_cell((12, 12), 1.0, 0.5, 100, 10, 20),
            ],
            tail_u: 0.0,
            target_width_cells: 1.0,
            tip_width_cap_cells: COMET_MIN_RESOLVABLE_WIDTH,
            transverse_width_penalty: 0.0,
        };

        let solved =
            solve_ribbon_dp(&[slice0, slice1, slice2], 1024).expect("ribbon path should solve");
        assert_eq!(solved.len(), 3);
        assert_eq!(solved[0].run_start_key(), Some(0));
        assert_eq!(solved[1].run_start_key(), Some(0));
        assert_eq!(solved[2].run_start_key(), Some(0));
    }

    #[test]
    fn ribbon_slice_preserves_multiple_non_empty_candidates_per_cell() {
        let frame = with_block_aspect_ratio(&base_frame(), 1.0);
        let centerline = vec![CenterSample {
            pos: Point {
                row: 10.5,
                col: 10.5,
            },
            tangent_row: 0.0,
            tangent_col: 1.0,
        }];
        let candidates = BTreeMap::from([(
            (10_i64, 10_i64),
            ordered_candidates(vec![
                CellCandidate {
                    state: Some(decoded_state(DecodedGlyph::Block, 8)),
                    unary_cost: 0,
                },
                CellCandidate {
                    state: Some(decoded_state(DecodedGlyph::Matrix(0x3), 8)),
                    unary_cost: 20,
                },
                CellCandidate {
                    state: None,
                    unary_cost: 1_000,
                },
            ]),
        )]);

        let slices = build_ribbon_slices(&centerline, &candidates, &frame);

        assert_eq!(slices.len(), 1);
        assert_eq!(slices[0].cells.len(), 1);
        assert_eq!(slices[0].cells[0].non_empty_candidates.len(), 2);
    }

    #[test]
    fn ribbon_dp_can_choose_second_best_local_candidate_for_seam_consistency() {
        let block = decoded_state(DecodedGlyph::Block, 8);
        let matrix = decoded_state(DecodedGlyph::Matrix(0x3), 8);
        let slice0 = RibbonSlice {
            cells: vec![slice_cell_with_candidates(
                (10, 10),
                0.0,
                0.5,
                1_000,
                vec![CellCandidate {
                    state: Some(block),
                    unary_cost: 0,
                }],
            )],
            tail_u: 1.0,
            target_width_cells: 1.0,
            tip_width_cap_cells: COMET_MIN_RESOLVABLE_WIDTH,
            transverse_width_penalty: 0.0,
        };
        let slice1 = RibbonSlice {
            cells: vec![slice_cell_with_candidates(
                (10, 10),
                0.0,
                0.5,
                1_000,
                vec![
                    CellCandidate {
                        state: Some(matrix),
                        unary_cost: 0,
                    },
                    CellCandidate {
                        state: Some(block),
                        unary_cost: 120,
                    },
                ],
            )],
            tail_u: 0.5,
            target_width_cells: 1.0,
            tip_width_cap_cells: COMET_MIN_RESOLVABLE_WIDTH,
            transverse_width_penalty: 0.0,
        };
        let slice2 = RibbonSlice {
            cells: vec![slice_cell_with_candidates(
                (10, 10),
                0.0,
                0.5,
                1_000,
                vec![CellCandidate {
                    state: Some(block),
                    unary_cost: 0,
                }],
            )],
            tail_u: 0.0,
            target_width_cells: 1.0,
            tip_width_cap_cells: COMET_MIN_RESOLVABLE_WIDTH,
            transverse_width_penalty: 0.0,
        };

        let solved = solve_ribbon_dp(&[slice0, slice1.clone(), slice2], 1024)
            .expect("ribbon path should solve");

        assert_eq!(state_for_slice_cell(&slice1, solved[1], 0), Some(block));
    }

    #[test]
    fn ribbon_transition_cost_matches_linear_coordinate_scan_baseline() {
        let block = decoded_state(DecodedGlyph::Block, 8);
        let matrix = decoded_state(DecodedGlyph::Matrix(0x3), 8);
        let dim_block = decoded_state(DecodedGlyph::Block, 6);

        let previous_slice = RibbonSlice {
            cells: vec![
                slice_cell_with_candidates(
                    (10, 10),
                    -1.0,
                    0.5,
                    800,
                    vec![CellCandidate {
                        state: Some(block),
                        unary_cost: 0,
                    }],
                ),
                slice_cell_with_candidates(
                    (10, 11),
                    0.0,
                    0.5,
                    800,
                    vec![CellCandidate {
                        state: Some(matrix),
                        unary_cost: 0,
                    }],
                ),
                slice_cell_with_candidates(
                    (10, 12),
                    1.0,
                    0.5,
                    800,
                    vec![CellCandidate {
                        state: Some(dim_block),
                        unary_cost: 0,
                    }],
                ),
            ],
            tail_u: 0.8,
            target_width_cells: 2.0,
            tip_width_cap_cells: COMET_MIN_RESOLVABLE_WIDTH,
            transverse_width_penalty: 0.0,
        };
        let next_slice = RibbonSlice {
            cells: vec![
                slice_cell_with_candidates(
                    (10, 11),
                    -0.5,
                    0.5,
                    800,
                    vec![
                        CellCandidate {
                            state: Some(block),
                            unary_cost: 0,
                        },
                        CellCandidate {
                            state: Some(matrix),
                            unary_cost: 50,
                        },
                    ],
                ),
                slice_cell_with_candidates(
                    (10, 12),
                    0.5,
                    0.5,
                    800,
                    vec![CellCandidate {
                        state: Some(dim_block),
                        unary_cost: 0,
                    }],
                ),
                slice_cell_with_candidates(
                    (10, 13),
                    1.5,
                    0.5,
                    800,
                    vec![CellCandidate {
                        state: Some(block),
                        unary_cost: 0,
                    }],
                ),
            ],
            tail_u: 0.4,
            target_width_cells: 2.0,
            tip_width_cap_cells: COMET_MIN_RESOLVABLE_WIDTH,
            transverse_width_penalty: 0.0,
        };

        let previous_state = SliceState::with_run(
            RunSpan::try_new(0, 2).expect("valid run"),
            [0; RIBBON_MAX_RUN_LENGTH],
            0,
        );
        let mut next_offsets = [0; RIBBON_MAX_RUN_LENGTH];
        next_offsets[0] = 1;
        let next_state =
            SliceState::with_run(RunSpan::try_new(0, 2).expect("valid run"), next_offsets, 0);

        assert_eq!(
            transition_cost(
                &previous_slice,
                previous_state,
                &next_slice,
                next_state,
                1024
            ),
            transition_cost_linear_baseline(
                &previous_slice,
                previous_state,
                &next_slice,
                next_state,
                1024,
            )
        );
    }
}

mod projected_span_geometry {
    use super::*;

    #[test]
    fn projected_run_width_ignores_along_axis_duplicates() {
        let slice = RibbonSlice {
            cells: vec![
                slice_cell((10, 10), 0.0, 0.5, 100, 12, 0),
                slice_cell((10, 11), 0.0, 0.5, 100, 12, 0),
                slice_cell((11, 10), 1.0, 0.5, 100, 12, 0),
            ],
            tail_u: 0.5,
            target_width_cells: 1.0,
            tip_width_cap_cells: COMET_MIN_RESOLVABLE_WIDTH,
            transverse_width_penalty: 0.0,
        };

        let duplicated_band = SliceState::with_run(
            RunSpan::try_new(0, 1).expect("valid run"),
            [0; RIBBON_MAX_RUN_LENGTH],
            0,
        );
        let two_band_span = SliceState::with_run(
            RunSpan::try_new(0, 2).expect("valid run"),
            [0; RIBBON_MAX_RUN_LENGTH],
            0,
        );

        assert!(
            (run_width_cells(&slice, duplicated_band) - 1.0).abs() < 1.0e-3,
            "same-normal duplicates should not widen the slice"
        );
        assert!(
            (run_width_cells(&slice, two_band_span) - 2.0).abs() < 1.0e-3,
            "projected width should only grow with actual cross-track span"
        );
    }

    #[test]
    fn projected_run_width_matches_horizontal_and_vertical_duplicate_layouts() {
        let horizontal_slice = RibbonSlice {
            cells: vec![
                slice_cell((10, 10), 0.0, 0.5, 100, 12, 0),
                slice_cell((10, 11), 0.0, 0.5, 100, 12, 0),
                slice_cell((11, 10), 1.0, 0.5, 100, 12, 0),
            ],
            tail_u: 0.5,
            target_width_cells: 1.0,
            tip_width_cap_cells: COMET_MIN_RESOLVABLE_WIDTH,
            transverse_width_penalty: 0.0,
        };
        let vertical_slice = RibbonSlice {
            cells: vec![
                slice_cell((10, 10), 0.0, 0.5, 100, 12, 0),
                slice_cell((11, 10), 0.0, 0.5, 100, 12, 0),
                slice_cell((10, 11), 1.0, 0.5, 100, 12, 0),
            ],
            tail_u: 0.5,
            target_width_cells: 1.0,
            tip_width_cap_cells: COMET_MIN_RESOLVABLE_WIDTH,
            transverse_width_penalty: 0.0,
        };

        let duplicate_bands = SliceState::with_run(
            RunSpan::try_new(0, 1).expect("valid run"),
            [0; RIBBON_MAX_RUN_LENGTH],
            0,
        );
        let two_band_span = SliceState::with_run(
            RunSpan::try_new(0, 2).expect("valid run"),
            [0; RIBBON_MAX_RUN_LENGTH],
            0,
        );

        assert!(
            (run_width_cells(&horizontal_slice, duplicate_bands)
                - run_width_cells(&vertical_slice, duplicate_bands))
            .abs()
                < 1.0e-3,
            "orientation should not change duplicate-band width"
        );
        assert!(
            (run_width_cells(&horizontal_slice, two_band_span)
                - run_width_cells(&vertical_slice, two_band_span))
            .abs()
                < 1.0e-3,
            "orientation should not change two-band span width"
        );
    }

    #[test]
    fn projected_span_value_object_orders_and_covers_bounds() {
        let lower = ProjectedSpanQ16::try_new(to_q16(-0.5), to_q16(0.5)).expect("ordered span");
        let upper = ProjectedSpanQ16::try_new(to_q16(0.5), to_q16(1.5)).expect("ordered span");
        let covered = lower.cover(upper);

        assert!((covered.width_cells() - 2.0).abs() < 1.0e-3);
    }
}

mod slice_state_enumeration {
    use super::*;

    #[test]
    fn build_slice_states_matches_reference_for_dense_candidate_slice() {
        let slice = dense_slice_with_candidate_fanout(6, 7);

        assert_eq!(
            build_slice_states(&slice, 1536),
            build_slice_states_reference(&slice, 1536)
        );
    }

    #[test]
    fn build_slice_states_peak_working_set_stays_within_top_k_cap() {
        let slice = dense_slice_with_candidate_fanout(6, 7);

        let (states, peak_len) = build_slice_states_with_peak_working_set(&slice, 1536);

        assert_eq!(states.len(), RIBBON_MAX_STATES_PER_SLICE);
        assert_eq!(states, build_slice_states_reference(&slice, 1536));
        assert!(
            peak_len <= RIBBON_MAX_STATES_PER_SLICE,
            "collector should retain at most the configured top-k working set, observed {peak_len}"
        );
    }
}

mod decode_path_selection_and_salience {
    use super::*;

    #[test]
    fn ribbon_decode_falls_back_to_local_without_centerline() {
        let high = DecodedCellState {
            glyph: DecodedGlyph::Block,
            level: HighlightLevel::from_raw_clamped(12),
        };
        let medium = DecodedCellState {
            glyph: DecodedGlyph::Block,
            level: HighlightLevel::from_raw_clamped(8),
        };
        let low = DecodedCellState {
            glyph: DecodedGlyph::Block,
            level: HighlightLevel::from_raw_clamped(4),
        };

        let mut candidates = BTreeMap::new();
        candidates.insert(
            (7, 9),
            vec![
                CellCandidate {
                    state: Some(high),
                    unary_cost: 10,
                },
                CellCandidate {
                    state: Some(low),
                    unary_cost: 20,
                },
                CellCandidate {
                    state: None,
                    unary_cost: 100,
                },
            ],
        );
        candidates.insert(
            (7, 10),
            vec![
                CellCandidate {
                    state: Some(medium),
                    unary_cost: 20,
                },
                CellCandidate {
                    state: Some(low),
                    unary_cost: 22,
                },
                CellCandidate {
                    state: None,
                    unary_cost: 120,
                },
            ],
        );
        candidates.insert(
            (8, 10),
            vec![
                CellCandidate {
                    state: Some(low),
                    unary_cost: 30,
                },
                CellCandidate {
                    state: None,
                    unary_cost: 15,
                },
            ],
        );

        let frame = base_frame();
        let baseline = decode_locally(&candidates);
        let decoded = decode_compiled_field(&candidates, &[], &frame);
        assert_eq!(decoded, baseline);
    }

    #[test]
    fn ribbon_decode_recovers_diagonal_chain_from_local_elbow() {
        let state = highlight_state(8);
        let make_candidates = |non_empty_cost: u64, empty_cost: u64| {
            let mut candidates = vec![
                CellCandidate {
                    state: Some(state),
                    unary_cost: non_empty_cost,
                },
                CellCandidate {
                    state: None,
                    unary_cost: empty_cost,
                },
            ];
            candidates.sort_by(|lhs, rhs| candidate_cmp(*lhs, *rhs, None));
            candidates
        };
        let candidates = BTreeMap::from([
            ((9_i64, 10_i64), make_candidates(20, 0)),
            ((10_i64, 10_i64), make_candidates(0, 100)),
            ((10_i64, 11_i64), make_candidates(20, 0)),
            ((11_i64, 11_i64), make_candidates(0, 100)),
            ((11_i64, 12_i64), make_candidates(0, 100)),
            ((12_i64, 12_i64), make_candidates(20, 0)),
        ]);
        let tangent = 1.0 / 2.0_f64.sqrt();
        let centerline = vec![
            CenterSample {
                pos: Point {
                    row: 10.0,
                    col: 10.5,
                },
                tangent_row: tangent,
                tangent_col: tangent,
            },
            CenterSample {
                pos: Point {
                    row: 11.0,
                    col: 11.5,
                },
                tangent_row: tangent,
                tangent_col: tangent,
            },
            CenterSample {
                pos: Point {
                    row: 12.0,
                    col: 12.5,
                },
                tangent_row: tangent,
                tangent_col: tangent,
            },
        ];

        let frame = with_block_aspect_ratio(&base_frame(), 1.0);
        let baseline = decode_locally(&candidates);
        let decoded = decode_compiled_field(&candidates, &centerline, &frame);

        assert_eq!(
            baseline.keys().copied().collect::<BTreeSet<_>>(),
            BTreeSet::from([(10_i64, 10_i64), (11_i64, 11_i64), (11_i64, 12_i64)]),
            "baseline should expose the locally cheapest elbow before spatial decode"
        );
        let decoded_cells = decoded.keys().copied().collect::<BTreeSet<_>>();
        assert!(
            decoded_cells.is_superset(&BTreeSet::from([
                (10_i64, 10_i64),
                (11_i64, 11_i64),
                (12_i64, 12_i64),
            ])),
            "spatial decode should recover the coherent diagonal chain: decoded={decoded_cells:?}"
        );
    }

    #[test]
    fn ribbon_decode_uses_pairwise_fallback_when_dp_solver_fails() {
        let state = highlight_state(8);
        let make_candidates = |non_empty_cost: u64, empty_cost: u64| {
            let mut candidates = vec![
                CellCandidate {
                    state: Some(state),
                    unary_cost: non_empty_cost,
                },
                CellCandidate {
                    state: None,
                    unary_cost: empty_cost,
                },
            ];
            candidates.sort_by(|lhs, rhs| candidate_cmp(*lhs, *rhs, None));
            candidates
        };
        let candidates = BTreeMap::from([
            ((9_i64, 10_i64), make_candidates(20, 0)),
            ((10_i64, 10_i64), make_candidates(0, 100)),
            ((10_i64, 11_i64), make_candidates(20, 0)),
            ((11_i64, 11_i64), make_candidates(0, 100)),
            ((11_i64, 12_i64), make_candidates(0, 100)),
            ((12_i64, 12_i64), make_candidates(20, 0)),
        ]);
        let tangent = 1.0 / 2.0_f64.sqrt();
        let centerline = vec![
            CenterSample {
                pos: Point {
                    row: 10.0,
                    col: 10.5,
                },
                tangent_row: tangent,
                tangent_col: tangent,
            },
            CenterSample {
                pos: Point {
                    row: 11.0,
                    col: 11.5,
                },
                tangent_row: tangent,
                tangent_col: tangent,
            },
            CenterSample {
                pos: Point {
                    row: 12.0,
                    col: 12.5,
                },
                tangent_row: tangent,
                tangent_col: tangent,
            },
        ];

        let frame = with_block_aspect_ratio(&base_frame(), 1.0);
        let baseline = decode_locally(&candidates);
        let slices = build_ribbon_slices(&centerline, &candidates, &frame);
        assert_eq!(
            select_decode_path(&baseline, &slices, sanitize_spatial_weight_q10(&frame)),
            DecodePathTrace::RibbonDp
        );

        let decoded = decode_compiled_field_with_solver(
            &BTreeMap::new(),
            &candidates,
            &centerline,
            &frame,
            |_, _| None,
        );

        assert_eq!(decoded.path, DecodePathTrace::RibbonDpSolveFailed);
        assert_eq!(
            decoded.cells,
            solve_pairwise_fallback(&candidates, sanitize_spatial_weight_q10(&frame))
        );
    }

    #[test]
    fn disconnected_support_still_uses_fallback_detector() {
        let state = DecodedCellState {
            glyph: DecodedGlyph::Block,
            level: HighlightLevel::from_raw_clamped(8),
        };
        let support = BTreeMap::from([
            ((10, 10), state),
            ((10, 11), state),
            ((14, 14), state),
            ((14, 15), state),
        ]);

        assert!(active_support_is_disconnected(&support));
    }

    #[test]
    fn disconnected_support_detector_allows_single_component_thick_bar() {
        let state = DecodedCellState {
            glyph: DecodedGlyph::Block,
            level: HighlightLevel::from_raw_clamped(8),
        };
        let support = BTreeMap::from([
            ((10, 10), state),
            ((10, 11), state),
            ((10, 12), state),
            ((11, 10), state),
            ((11, 11), state),
            ((11, 12), state),
        ]);

        assert!(
            !active_support_is_disconnected(&support),
            "single-component straight bars should stay eligible for ribbon taper"
        );
    }

    #[test]
    fn disconnected_support_selects_pairwise_fallback_decode_path() {
        let frame = base_frame();
        let centerline = vec![
            CenterSample {
                pos: Point {
                    row: 10.5,
                    col: 10.5,
                },
                tangent_row: 0.0,
                tangent_col: 1.0,
            },
            CenterSample {
                pos: Point {
                    row: 14.5,
                    col: 14.5,
                },
                tangent_row: 0.0,
                tangent_col: 1.0,
            },
        ];
        let candidates = BTreeMap::from([
            (
                (10_i64, 10_i64),
                ordered_candidates(vec![
                    CellCandidate {
                        state: Some(highlight_state(8)),
                        unary_cost: 0,
                    },
                    CellCandidate {
                        state: None,
                        unary_cost: 40_000,
                    },
                ]),
            ),
            (
                (10_i64, 11_i64),
                ordered_candidates(vec![
                    CellCandidate {
                        state: Some(highlight_state(8)),
                        unary_cost: 0,
                    },
                    CellCandidate {
                        state: None,
                        unary_cost: 40_000,
                    },
                ]),
            ),
            (
                (14_i64, 14_i64),
                ordered_candidates(vec![
                    CellCandidate {
                        state: Some(highlight_state(8)),
                        unary_cost: 0,
                    },
                    CellCandidate {
                        state: None,
                        unary_cost: 40_000,
                    },
                ]),
            ),
            (
                (14_i64, 15_i64),
                ordered_candidates(vec![
                    CellCandidate {
                        state: Some(highlight_state(8)),
                        unary_cost: 0,
                    },
                    CellCandidate {
                        state: None,
                        unary_cost: 40_000,
                    },
                ]),
            ),
        ]);

        let baseline = decode_locally(&candidates);
        let slices = build_ribbon_slices(&centerline, &candidates, &frame);

        assert_eq!(
            select_decode_path(&baseline, &slices, sanitize_spatial_weight_q10(&frame)),
            DecodePathTrace::PairwiseFallbackDisconnected
        );
        assert_eq!(
            decode_compiled_field(&candidates, &centerline, &frame),
            solve_pairwise_fallback(&candidates, sanitize_spatial_weight_q10(&frame))
        );
    }

    #[test]
    fn sparse_undecodable_gap_keeps_ribbon_path_and_destination_salience() {
        let frame = with_block_aspect_ratio(&base_frame(), 1.0);
        let tangent = 1.0 / 2.0_f64.sqrt();
        let centerline = vec![
            CenterSample {
                pos: Point {
                    row: 10.5,
                    col: 10.5,
                },
                tangent_row: tangent,
                tangent_col: tangent,
            },
            CenterSample {
                pos: Point {
                    row: 11.5,
                    col: 11.5,
                },
                tangent_row: tangent,
                tangent_col: tangent,
            },
            CenterSample {
                pos: Point {
                    row: 12.5,
                    col: 12.5,
                },
                tangent_row: tangent,
                tangent_col: tangent,
            },
            CenterSample {
                pos: Point {
                    row: 13.5,
                    col: 13.5,
                },
                tangent_row: tangent,
                tangent_col: tangent,
            },
            CenterSample {
                pos: Point {
                    row: 14.5,
                    col: 14.5,
                },
                tangent_row: tangent,
                tangent_col: tangent,
            },
        ];
        let candidates = BTreeMap::from([
            (
                (10_i64, 10_i64),
                ordered_candidates(vec![
                    CellCandidate {
                        state: Some(highlight_state(4)),
                        unary_cost: 0,
                    },
                    CellCandidate {
                        state: None,
                        unary_cost: 40_000,
                    },
                ]),
            ),
            (
                (11_i64, 11_i64),
                ordered_candidates(vec![
                    CellCandidate {
                        state: Some(highlight_state(4)),
                        unary_cost: 0,
                    },
                    CellCandidate {
                        state: None,
                        unary_cost: 40_000,
                    },
                ]),
            ),
            (
                (12_i64, 12_i64),
                ordered_candidates(vec![CellCandidate {
                    state: None,
                    unary_cost: 0,
                }]),
            ),
            (
                (13_i64, 13_i64),
                ordered_candidates(vec![
                    CellCandidate {
                        state: Some(highlight_state(12)),
                        unary_cost: 0,
                    },
                    CellCandidate {
                        state: None,
                        unary_cost: 40_000,
                    },
                ]),
            ),
            (
                (14_i64, 14_i64),
                ordered_candidates(vec![
                    CellCandidate {
                        state: Some(highlight_state(12)),
                        unary_cost: 0,
                    },
                    CellCandidate {
                        state: None,
                        unary_cost: 40_000,
                    },
                ]),
            ),
        ]);

        let baseline = decode_locally(&candidates);
        assert!(
            active_support_is_disconnected(&baseline),
            "fixture should look disconnected to the local baseline"
        );

        let decoded = decode_compiled_field_trace(&candidates, &centerline, &frame);
        let tail_level = decoded
            .cells
            .get(&(10_i64, 10_i64))
            .expect("tail cell should stay decoded")
            .level
            .value();
        let head_level = decoded
            .cells
            .get(&(14_i64, 14_i64))
            .expect("destination catch should stay decoded")
            .level
            .value();

        assert_eq!(decoded.path, DecodePathTrace::RibbonDp);
        assert!(
            !decoded.cells.contains_key(&(12_i64, 12_i64)),
            "undecodable bridge cells should remain empty rather than forcing a fallback path"
        );
        assert!(
            head_level > tail_level,
            "destination catch should remain more salient than the long bridge tail"
        );
    }

    #[test]
    fn destination_catch_prefers_brighter_head_state_over_slightly_cheaper_dim_state() {
        let frame = with_block_aspect_ratio(&base_frame(), 1.0);
        let tangent = 1.0 / 2.0_f64.sqrt();
        let centerline = vec![
            CenterSample {
                pos: Point {
                    row: 10.5,
                    col: 10.5,
                },
                tangent_row: tangent,
                tangent_col: tangent,
            },
            CenterSample {
                pos: Point {
                    row: 11.5,
                    col: 11.5,
                },
                tangent_row: tangent,
                tangent_col: tangent,
            },
            CenterSample {
                pos: Point {
                    row: 12.5,
                    col: 12.5,
                },
                tangent_row: tangent,
                tangent_col: tangent,
            },
            CenterSample {
                pos: Point {
                    row: 13.5,
                    col: 13.5,
                },
                tangent_row: tangent,
                tangent_col: tangent,
            },
            CenterSample {
                pos: Point {
                    row: 14.5,
                    col: 14.5,
                },
                tangent_row: tangent,
                tangent_col: tangent,
            },
        ];
        let candidates = BTreeMap::from([
            (
                (10_i64, 10_i64),
                ordered_candidates(vec![
                    CellCandidate {
                        state: Some(highlight_state(4)),
                        unary_cost: 0,
                    },
                    CellCandidate {
                        state: None,
                        unary_cost: 40_000,
                    },
                ]),
            ),
            (
                (11_i64, 11_i64),
                ordered_candidates(vec![
                    CellCandidate {
                        state: Some(highlight_state(4)),
                        unary_cost: 0,
                    },
                    CellCandidate {
                        state: None,
                        unary_cost: 40_000,
                    },
                ]),
            ),
            (
                (12_i64, 12_i64),
                ordered_candidates(vec![
                    CellCandidate {
                        state: Some(highlight_state(6)),
                        unary_cost: 0,
                    },
                    CellCandidate {
                        state: None,
                        unary_cost: 40_000,
                    },
                ]),
            ),
            (
                (13_i64, 13_i64),
                ordered_candidates(vec![
                    CellCandidate {
                        state: Some(highlight_state(6)),
                        unary_cost: 0,
                    },
                    CellCandidate {
                        state: Some(highlight_state(12)),
                        unary_cost: 48,
                    },
                    CellCandidate {
                        state: None,
                        unary_cost: 40_000,
                    },
                ]),
            ),
            (
                (14_i64, 14_i64),
                ordered_candidates(vec![
                    CellCandidate {
                        state: Some(highlight_state(6)),
                        unary_cost: 0,
                    },
                    CellCandidate {
                        state: Some(highlight_state(12)),
                        unary_cost: 48,
                    },
                    CellCandidate {
                        state: None,
                        unary_cost: 40_000,
                    },
                ]),
            ),
        ]);

        let decoded = decode_compiled_field_trace(&candidates, &centerline, &frame);
        let head_level = decoded
            .cells
            .get(&(14_i64, 14_i64))
            .expect("destination catch should stay decoded")
            .level
            .value();
        let tail_level = decoded
            .cells
            .get(&(10_i64, 10_i64))
            .expect("tail cell should stay decoded")
            .level
            .value();

        assert_eq!(decoded.path, DecodePathTrace::RibbonDp);
        assert_eq!(
            head_level, 12,
            "destination catch should keep the brighter head state even when it is slightly costlier"
        );
        assert!(
            head_level > tail_level,
            "head salience should dominate the bridge tail"
        );
    }

    #[test]
    fn oversized_ribbon_support_uses_pairwise_fallback_instead_of_local_baseline() {
        let frame = with_trail_thickness(&with_block_aspect_ratio(&base_frame(), 1.0), 4.0);
        let centerline = vec![
            CenterSample {
                pos: Point {
                    row: 10.5,
                    col: 10.0,
                },
                tangent_row: 0.0,
                tangent_col: 1.0,
            },
            CenterSample {
                pos: Point {
                    row: 10.5,
                    col: 11.0,
                },
                tangent_row: 0.0,
                tangent_col: 1.0,
            },
        ];
        let mut candidates = BTreeMap::<(i64, i64), Vec<CellCandidate>>::new();
        for row in 7_i64..=13_i64 {
            candidates.insert(
                (row, 9_i64),
                ordered_candidates(vec![
                    CellCandidate {
                        state: Some(highlight_state(2)),
                        unary_cost: 0,
                    },
                    CellCandidate {
                        state: Some(highlight_state(6)),
                        unary_cost: 100,
                    },
                    CellCandidate {
                        state: None,
                        unary_cost: 40_000,
                    },
                ]),
            );
            candidates.insert(
                (row, 10_i64),
                ordered_candidates(vec![
                    CellCandidate {
                        state: Some(highlight_state(6)),
                        unary_cost: 0,
                    },
                    CellCandidate {
                        state: Some(highlight_state(2)),
                        unary_cost: 100,
                    },
                    CellCandidate {
                        state: None,
                        unary_cost: 40_000,
                    },
                ]),
            );
            candidates.insert(
                (row, 11_i64),
                ordered_candidates(vec![
                    CellCandidate {
                        state: Some(highlight_state(2)),
                        unary_cost: 0,
                    },
                    CellCandidate {
                        state: Some(highlight_state(6)),
                        unary_cost: 100,
                    },
                    CellCandidate {
                        state: None,
                        unary_cost: 40_000,
                    },
                ]),
            );
        }

        let slices = build_ribbon_slices(&centerline, &candidates, &frame);
        assert!(
            ribbon_support_is_oversized(&slices),
            "test fixture should exceed the ribbon cross-section cap"
        );

        let baseline = decode_locally(&candidates);
        assert_eq!(
            select_decode_path(&baseline, &slices, sanitize_spatial_weight_q10(&frame)),
            DecodePathTrace::PairwiseFallbackOversized
        );
        let fallback = solve_pairwise_fallback(&candidates, sanitize_spatial_weight_q10(&frame));
        let decoded = decode_compiled_field(&candidates, &centerline, &frame);

        assert_eq!(decoded, fallback);
    }
}

mod ribbon_width_targets_and_taper {
    use super::*;

    #[test]
    fn cell_row_index_limits_same_row_queries_to_the_requested_column_window() {
        let cells = (0_i64..=200_i64)
            .map(|col| ((10_i64, col), col))
            .collect::<BTreeMap<_, _>>();
        let index = CellRowIndex::build(&cells);
        let mut visited = Vec::<(i64, i64)>::new();

        index.for_each_in_bounds(
            SliceSearchBounds {
                min_row: 10,
                max_row: 10,
                min_col: 99,
                max_col: 101,
            },
            |coord, _| visited.push(coord),
        );

        assert_eq!(visited, vec![(10, 99), (10, 100), (10, 101)]);
    }

    #[test]
    fn comet_taper_target_is_monotonic_from_head_to_tip() {
        let head_width = default_head_width_cells(&base_frame());
        let samples = (0..=100)
            .map(|index| {
                let u = index as f64 / 100.0;
                comet_target_width_cells(head_width, u)
            })
            .collect::<Vec<_>>();

        for widths in samples.windows(2) {
            assert!(
                widths[0] + 1.0e-9 >= widths[1],
                "taper should be monotonic: {widths:?}"
            );
        }
        assert!(
            samples
                .last()
                .is_some_and(|tip| *tip >= COMET_MIN_RESOLVABLE_WIDTH)
        );
    }

    #[test]
    fn slice_target_width_tracks_compiled_support_not_only_global_thickness() {
        let frame = with_block_aspect_ratio(&with_trail_thickness(&base_frame(), 1.0), 1.0);
        let centerline = vec![CenterSample {
            pos: Point {
                row: 10.5,
                col: 10.5,
            },
            tangent_row: 1.0,
            tangent_col: 0.0,
        }];
        let full_width_tile = tile_for_column_span(0, latent_field::MICRO_W - 1, 0x0FFF);
        let wide_compiled = BTreeMap::from([
            (
                (10_i64, 10_i64),
                latent_field::CompiledCell {
                    tile: full_width_tile,
                    age: AgeMoment::default(),
                },
            ),
            (
                (10_i64, 11_i64),
                latent_field::CompiledCell {
                    tile: full_width_tile,
                    age: AgeMoment::default(),
                },
            ),
        ]);
        let narrow_compiled = compiled_single_cell(
            tile_for_column_span(0, latent_field::MICRO_W - 1, 0x0FFF),
            AgeMoment::default(),
        );
        let wide_candidates =
            build_cell_candidates(&wide_compiled, &BTreeMap::new(), frame.color_levels, 0.0, 5);
        let narrow_candidates = build_cell_candidates(
            &narrow_compiled,
            &BTreeMap::new(),
            frame.color_levels,
            0.0,
            5,
        );

        let wide_slices = build_ribbon_slices_with_compiled(
            &centerline,
            &wide_compiled,
            &wide_candidates,
            &frame,
        );
        let narrow_slices = build_ribbon_slices_with_compiled(
            &centerline,
            &narrow_compiled,
            &narrow_candidates,
            &frame,
        );

        assert_eq!(wide_slices.len(), 1);
        assert_eq!(narrow_slices.len(), 1);
        assert!(
            wide_slices[0].target_width_cells > narrow_slices[0].target_width_cells,
            "wider latent support should produce a wider target: wide={} narrow={}",
            wide_slices[0].target_width_cells,
            narrow_slices[0].target_width_cells,
        );
    }

    #[test]
    fn ribbon_dp_monotonic_prior_blocks_one_cell_tail_rewiden_after_head_width_change() {
        let tail_slice = RibbonSlice {
            cells: vec![
                slice_cell((10, 10), 0.0, 0.5, 1_000, 12, 0),
                slice_cell((10, 11), 1.0, 0.5, 0, 12, 0),
            ],
            tail_u: 1.0,
            target_width_cells: 2.0,
            tip_width_cap_cells: COMET_MIN_RESOLVABLE_WIDTH,
            transverse_width_penalty: 0.0,
        };
        let head_slice = RibbonSlice {
            cells: vec![slice_cell((11, 10), 0.0, 0.5, 1_000, 12, 0)],
            tail_u: 0.0,
            target_width_cells: 1.0,
            tip_width_cap_cells: COMET_MIN_RESOLVABLE_WIDTH,
            transverse_width_penalty: 0.0,
        };

        let solved = solve_ribbon_dp(&[tail_slice.clone(), head_slice.clone()], 1024)
            .expect("ribbon path should solve");

        assert!(
            (run_width_cells(&tail_slice, solved[0]) - 1.0).abs() < 1.0e-3,
            "monotonic prior should keep the tail from re-widening even when its target width grows"
        );
        assert!(
            (run_width_cells(&head_slice, solved[1]) - 1.0).abs() < 1.0e-3,
            "head slice should remain narrow in the control fixture"
        );
    }

    #[test]
    fn slice_taper_targets_stay_stable_across_aspect_ratios_when_support_width_is_unchanged() {
        let frame = with_trail_thickness(&base_frame(), 1.0);
        let centerline = vec![
            CenterSample {
                pos: Point {
                    row: 10.5,
                    col: 10.5,
                },
                tangent_row: 1.0,
                tangent_col: 0.0,
            },
            CenterSample {
                pos: Point {
                    row: 11.5,
                    col: 10.5,
                },
                tangent_row: 1.0,
                tangent_col: 0.0,
            },
            CenterSample {
                pos: Point {
                    row: 12.5,
                    col: 10.5,
                },
                tangent_row: 1.0,
                tangent_col: 0.0,
            },
        ];
        let compiled = BTreeMap::from([
            (
                (10_i64, 10_i64),
                latent_field::CompiledCell {
                    tile: tile_for_column_span(0, latent_field::MICRO_W - 1, 0x0FFF),
                    age: AgeMoment::default(),
                },
            ),
            (
                (11_i64, 10_i64),
                latent_field::CompiledCell {
                    tile: tile_for_column_span(0, latent_field::MICRO_W - 1, 0x0FFF),
                    age: AgeMoment::default(),
                },
            ),
            (
                (12_i64, 10_i64),
                latent_field::CompiledCell {
                    tile: tile_for_column_span(0, latent_field::MICRO_W - 1, 0x0FFF),
                    age: AgeMoment::default(),
                },
            ),
        ]);
        let candidates =
            build_cell_candidates(&compiled, &BTreeMap::new(), frame.color_levels, 0.0, 5);
        let aspect_one = with_block_aspect_ratio(&frame, 1.0);
        let aspect_two = with_block_aspect_ratio(&frame, 2.0);

        let aspect_one_slices =
            build_ribbon_slices_with_compiled(&centerline, &compiled, &candidates, &aspect_one);
        let aspect_two_slices =
            build_ribbon_slices_with_compiled(&centerline, &compiled, &candidates, &aspect_two);

        assert_eq!(aspect_one_slices.len(), aspect_two_slices.len());
        for (aspect_one_slice, aspect_two_slice) in
            aspect_one_slices.iter().zip(aspect_two_slices.iter())
        {
            assert!(
                (aspect_one_slice.target_width_cells - aspect_two_slice.target_width_cells).abs()
                    < 1.0e-6,
                "taper targets should remain stable when aspect ratio changes without changing cross-track support width: aspect1={} aspect2={}",
                aspect_one_slice.target_width_cells,
                aspect_two_slice.target_width_cells,
            );
        }
    }
}

mod staged_deposits_and_metric_projection {
    use super::*;

    fn staged_deposit_band_masses(frame: &RenderFrame) -> (u32, u32, u32) {
        let mut state = PlannerState {
            last_pose: Some(pose_for_frame(&base_frame())),
            ..PlannerState::default()
        };
        stage_deposited_samples(&mut state, frame);

        let mass_for = |band| {
            state
                .history
                .iter()
                .filter(|slice| slice.band == band)
                .map(|slice| slice.intensity_q16)
                .sum::<u32>()
        };

        (
            mass_for(latent_field::TailBand::Sheath),
            mass_for(latent_field::TailBand::Core),
            mass_for(latent_field::TailBand::Filament),
        )
    }

    #[test]
    fn staged_deposit_band_masses_shift_with_speed() {
        let mut moving_frame = base_frame();
        set_frame_corners(
            &mut moving_frame,
            [
                Point {
                    row: 10.0,
                    col: 14.0,
                },
                Point {
                    row: 10.0,
                    col: 15.0,
                },
                Point {
                    row: 11.0,
                    col: 15.0,
                },
                Point {
                    row: 11.0,
                    col: 14.0,
                },
            ],
        );

        let mut stationary_frame = base_frame();
        stationary_frame.step_samples = vec![sample_for_corners(stationary_frame.corners)].into();

        let (slow_sheath_mass, slow_core_mass, slow_filament_mass) =
            staged_deposit_band_masses(&stationary_frame);
        let (fast_sheath_mass, fast_core_mass, fast_filament_mass) =
            staged_deposit_band_masses(&moving_frame);

        assert!(
            fast_sheath_mass > slow_sheath_mass,
            "head-adjacent sheath band should still respond to speed"
        );
        assert!(fast_core_mass > slow_core_mass);
        assert_eq!(
            fast_filament_mass, slow_filament_mass,
            "far-tail filament band should stay speed-invariant"
        );
    }

    #[test]
    fn staged_deposit_records_bbox_dt_and_arc_length_metadata() {
        let mut moving_frame = base_frame();
        set_frame_corners(
            &mut moving_frame,
            [
                Point {
                    row: 10.0,
                    col: 12.0,
                },
                Point {
                    row: 10.0,
                    col: 13.0,
                },
                Point {
                    row: 11.0,
                    col: 13.0,
                },
                Point {
                    row: 11.0,
                    col: 12.0,
                },
            ],
        );

        let mut state = PlannerState {
            last_pose: Some(pose_for_frame(&base_frame())),
            ..PlannerState::default()
        };

        stage_deposited_samples(&mut state, &moving_frame);

        let core_slice = state
            .history
            .iter()
            .find(|slice| slice.band == latent_field::TailBand::Core)
            .expect("core slice should be staged");
        assert!(core_slice.dt_ms_q16 > 0);
        assert!(core_slice.arc_len_q16.value() > 0);
        assert!(!core_slice.microtiles.is_empty());
        assert!(
            core_slice
                .microtiles
                .keys()
                .all(|coord| core_slice.bbox.contains(*coord))
        );
        assert_eq!(state.arc_len_q16, core_slice.arc_len_q16);
    }

    #[test]
    fn staged_deposit_advances_once_per_render_step_sample() {
        let mut moving_frame = base_frame();
        let intermediate = [
            Point {
                row: 10.0,
                col: 11.0,
            },
            Point {
                row: 10.0,
                col: 12.0,
            },
            Point {
                row: 11.0,
                col: 12.0,
            },
            Point {
                row: 11.0,
                col: 11.0,
            },
        ];
        let final_corners = [
            Point {
                row: 10.0,
                col: 12.0,
            },
            Point {
                row: 10.0,
                col: 13.0,
            },
            Point {
                row: 11.0,
                col: 13.0,
            },
            Point {
                row: 11.0,
                col: 12.0,
            },
        ];
        moving_frame.corners = final_corners;
        moving_frame.target_corners = final_corners;
        moving_frame.step_samples = vec![
            sample_for_corners(intermediate),
            sample_for_corners(final_corners),
        ]
        .into();

        let mut state = PlannerState {
            last_pose: Some(pose_for_frame(&base_frame())),
            ..PlannerState::default()
        };

        stage_deposited_samples(&mut state, &moving_frame);

        assert_eq!(state.step_index.value(), 2);
        assert_eq!(state.center_history.len(), 2);
        assert_eq!(
            state
                .history
                .iter()
                .filter(|slice| slice.band == latent_field::TailBand::Core)
                .count(),
            2
        );
        assert_eq!(
            state.center_history.back().map(|sample| sample.pos),
            Some(pose_center(&final_corners))
        );
    }

    #[test]
    fn centerline_resample_uses_display_metric_arc_length() {
        let mut history = VecDeque::new();
        history.push_back(CenterPathSample {
            step_index: StepIndex::new(1),
            pos: Point {
                row: 10.0,
                col: 10.0,
            },
        });
        history.push_back(CenterPathSample {
            step_index: StepIndex::new(2),
            pos: Point {
                row: 11.0,
                col: 10.0,
            },
        });

        let aspect_one = resample_centerline(&history, 1.0, 1.0);
        let aspect_two = resample_centerline(&history, 1.0, 2.0);

        assert_eq!(aspect_one.len(), 2);
        assert_eq!(aspect_two.len(), 3);
    }

    #[test]
    fn centerline_curvature_uses_display_metric_arc_term() {
        let centerline = vec![
            CenterSample {
                pos: Point {
                    row: 10.0,
                    col: 10.0,
                },
                tangent_row: 1.0,
                tangent_col: 0.0,
            },
            CenterSample {
                pos: Point {
                    row: 11.0,
                    col: 10.0,
                },
                tangent_row: 1.0,
                tangent_col: 0.0,
            },
            CenterSample {
                pos: Point {
                    row: 11.0,
                    col: 11.0,
                },
                tangent_row: 0.0,
                tangent_col: 1.0,
            },
        ];

        let aspect_one = centerline_curvature(&centerline, 1, 1.0);
        let aspect_two = centerline_curvature(&centerline, 1, 2.0);
        assert!(aspect_two < aspect_one);
    }

    #[test]
    fn ribbon_projection_uses_display_metric_offsets() {
        let frame = base_frame();
        let aspect_one = with_block_aspect_ratio(&frame, 1.0);
        let aspect_two = with_block_aspect_ratio(&frame, 2.0);
        let centerline = vec![CenterSample {
            pos: Point {
                row: 9.75,
                col: 10.5,
            },
            tangent_row: 0.0,
            tangent_col: 1.0,
        }];
        let state = DecodedCellState {
            glyph: DecodedGlyph::Block,
            level: HighlightLevel::from_raw_clamped(12),
        };
        let candidates = BTreeMap::from([(
            (10_i64, 10_i64),
            vec![
                CellCandidate {
                    state: Some(state),
                    unary_cost: 0,
                },
                CellCandidate {
                    state: None,
                    unary_cost: 10,
                },
            ],
        )]);

        let projected_with_aspect_one = build_ribbon_slices(&centerline, &candidates, &aspect_one);
        let projected_with_aspect_two = build_ribbon_slices(&centerline, &candidates, &aspect_two);

        assert_eq!(projected_with_aspect_one.len(), 1);
        assert_eq!(projected_with_aspect_one[0].cells.len(), 1);
        assert!(projected_with_aspect_two.is_empty());
    }
}
