use super::*;
use crate::test_support::proptest::positive_aspect_ratio;
use crate::test_support::proptest::pure_config;
use crate::test_support::proptest::staged_render_step_samples;
use pretty_assertions::assert_eq;
use proptest::collection::vec;
use proptest::prelude::*;
use std::collections::VecDeque;

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

fn expected_resample_len(
    history: &VecDeque<CenterPathSample>,
    spacing: f64,
    block_aspect_ratio: f64,
) -> usize {
    if history.is_empty() {
        return 0;
    }
    if history.len() == 1 {
        return 1;
    }

    let total_len = history
        .iter()
        .zip(history.iter().skip(1))
        .map(|(start, end)| start.pos.display_distance(end.pos, block_aspect_ratio))
        .sum::<f64>();
    if total_len <= f64::EPSILON {
        return 1;
    }

    let safe_spacing = if spacing.is_finite() {
        spacing.max(0.125)
    } else {
        RIBBON_SAMPLE_SPACING_CELLS
    };
    (total_len / safe_spacing).ceil() as usize + 1
}

fn vertical_history(
    row_deltas: &[u8],
    start_row: i64,
    start_col: i64,
) -> VecDeque<CenterPathSample> {
    let mut history = VecDeque::new();
    let mut row = start_row as f64;
    history.push_back(CenterPathSample {
        step_index: StepIndex::new(1),
        pos: RenderPoint {
            row,
            col: start_col as f64,
        },
    });

    for (index, delta) in row_deltas.iter().copied().enumerate() {
        row += f64::from(delta);
        history.push_back(CenterPathSample {
            step_index: StepIndex::new(u64::try_from(index + 2).unwrap_or(u64::MAX)),
            pos: RenderPoint {
                row,
                col: start_col as f64,
            },
        });
    }

    history
}

fn straight_centerline(
    sample_count: usize,
    start_row: i64,
    start_col: i64,
    step: u8,
    vertical: bool,
) -> Vec<CenterSample> {
    (0..sample_count)
        .map(|index| {
            let offset = f64::from(step) * index as f64;
            let (row, col, tangent_row, tangent_col) = if vertical {
                (
                    start_row as f64 + offset + 0.5,
                    start_col as f64 + 0.5,
                    1.0,
                    0.0,
                )
            } else {
                (
                    start_row as f64 + 0.5,
                    start_col as f64 + offset + 0.5,
                    0.0,
                    1.0,
                )
            };

            CenterSample {
                pos: RenderPoint { row, col },
                tangent_row,
                tangent_col,
            }
        })
        .collect()
}

#[test]
fn staged_deposit_band_masses_shift_with_speed() {
    let mut moving_frame = base_frame();
    set_frame_corners(
        &mut moving_frame,
        [
            RenderPoint {
                row: 10.0,
                col: 14.0,
            },
            RenderPoint {
                row: 10.0,
                col: 15.0,
            },
            RenderPoint {
                row: 11.0,
                col: 15.0,
            },
            RenderPoint {
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
fn stage_deposited_samples_reuses_retained_sweep_scratch_between_frames() {
    let viewport = test_viewport();
    let first = render_frame_to_plan(
        &single_sample_frame(12, 14),
        PlannerState::default(),
        viewport,
    )
    .next_state;
    let first_capacities = (
        first.sweep_scratch.row_projection_capacity(),
        first.sweep_scratch.col_projection_capacity(),
        first.sweep_scratch.tile_capacity(),
    );
    assert!(
        first_capacities.0 > 0 && first_capacities.1 > 0 && first_capacities.2 > 0,
        "seeded planner state should retain populated sweep scratch capacity"
    );

    let second = render_frame_to_plan(&single_sample_frame(12, 14), first, viewport).next_state;

    assert_eq!(
        (
            second.sweep_scratch.row_projection_capacity(),
            second.sweep_scratch.col_projection_capacity(),
            second.sweep_scratch.tile_capacity(),
        ),
        first_capacities,
    );
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_stage_deposited_samples_advance_per_step_and_preserve_metadata(
        step_samples in staged_render_step_samples(4),
    ) {
        let final_corners = step_samples
            .last()
            .map(|sample| sample.corners)
            .expect("strategy always generates at least one step sample");
        let mut frame = base_frame();
        frame.corners = final_corners;
        frame.target_corners = final_corners;
        frame.step_samples = step_samples.clone().into();

        let mut state = PlannerState {
            last_pose: Some(pose_for_frame(&base_frame())),
            ..PlannerState::default()
        };

        stage_deposited_samples(&mut state, &frame);

        let core_slices = state
            .history
            .iter()
            .filter(|slice| slice.band == latent_field::TailBand::Core)
            .collect::<Vec<_>>();

        prop_assert_eq!(
            state.step_index.value(),
            u64::try_from(step_samples.len()).unwrap_or(u64::MAX),
        );
        prop_assert_eq!(state.center_history.len(), step_samples.len());
        prop_assert!(core_slices.len() <= step_samples.len());
        prop_assert_eq!(
            state.center_history.back().map(|sample| sample.pos),
            Some(crate::position::corners_center(&final_corners)),
        );

        let mut previous_arc_len = 0_u32;
        for core_slice in &core_slices {
            let sample_index = usize::try_from(core_slice.step_index.value().saturating_sub(1))
                .unwrap_or(usize::MAX);
            let sample = step_samples
                .get(sample_index)
                .expect("core slice step index should map to a staged step sample");
            prop_assert_eq!(
                core_slice.dt_ms_q16,
                latent_field::q16_from_non_negative(sample.dt_ms),
            );
            prop_assert!(!core_slice.microtiles.is_empty());
            prop_assert!(
                core_slice
                    .microtiles
                    .keys()
                    .all(|coord| core_slice.bbox.contains(*coord))
            );
            prop_assert!(core_slice.arc_len_q16.value() >= previous_arc_len);
            previous_arc_len = core_slice.arc_len_q16.value();
        }
        prop_assert_eq!(
            state.arc_len_q16,
            core_slices
                .last()
                .map(|slice| slice.arc_len_q16)
                .unwrap_or(ArcLenQ16::ZERO),
        );
    }

    #[test]
    fn prop_centerline_resample_uses_display_metric_path_length_for_vertical_histories(
        row_deltas in vec(1_u8..=4_u8, 1..=4),
        spacing in 0.25_f64..=1.5_f64,
        aspect_one in positive_aspect_ratio(),
        aspect_two in positive_aspect_ratio(),
        start_row in 8_i64..=16_i64,
        start_col in 8_i64..=16_i64,
    ) {
        let history = vertical_history(&row_deltas, start_row, start_col);
        let resampled_one = resample_centerline(&history, spacing, aspect_one);
        let resampled_two = resample_centerline(&history, spacing, aspect_two);

        prop_assert_eq!(
            resampled_one.len(),
            expected_resample_len(&history, spacing, aspect_one),
        );
        prop_assert_eq!(
            resampled_two.len(),
            expected_resample_len(&history, spacing, aspect_two),
        );

        let (smaller_len, larger_len) = if aspect_one <= aspect_two {
            (resampled_one.len(), resampled_two.len())
        } else {
            (resampled_two.len(), resampled_one.len())
        };
        prop_assert!(larger_len >= smaller_len);
    }

    #[test]
    fn prop_local_query_envelope_matches_union_of_straight_centerline_bounds_and_previous_halo(
        sample_count in 0_usize..=5_usize,
        start_row in 8_i64..=14_i64,
        start_col in 8_i64..=14_i64,
        step in 1_u8..=3_u8,
        vertical in any::<bool>(),
        previous_entries in vec((8_i64..=18_i64, 8_i64..=18_i64, 1_u8..=16_u8), 0..=6),
        previous_cell_halo in 0_i64..=4_i64,
        aspect_ratio in positive_aspect_ratio(),
        trail_thickness in 0.5_f64..=2.5_f64,
    ) {
        let frame = with_block_aspect_ratio(
            &with_trail_thickness(&base_frame(), trail_thickness),
            aspect_ratio,
        );
        let centerline = straight_centerline(sample_count, start_row, start_col, step, vertical);
        let previous_cells = previous_entries
            .into_iter()
            .map(|(row, col, level)| ((row, col), highlight_state(u32::from(level))))
            .collect::<BTreeMap<_, _>>();

        let mut expected: Option<SliceSearchBounds> = None;
        for (sample_index, sample) in centerline.iter().copied().enumerate() {
            let bounds = super::local_envelope::ribbon_slice_search_bounds(
                sample,
                &frame,
                super::local_envelope::centerline_tail_u(sample_index, centerline.len()),
                0.0,
            );
            if let Some(existing) = &mut expected {
                existing.min_row = existing.min_row.min(bounds.min_row);
                existing.max_row = existing.max_row.max(bounds.max_row);
                existing.min_col = existing.min_col.min(bounds.min_col);
                existing.max_col = existing.max_col.max(bounds.max_col);
            } else {
                expected = Some(bounds);
            }
        }

        let halo = previous_cell_halo.max(0);
        let mut previous_coords = previous_cells.keys().copied();
        if let Some((first_row, first_col)) = previous_coords.next() {
            let mut previous_bounds = SliceSearchBounds::new(first_row, first_row, first_col, first_col);
            for (row, col) in previous_coords {
                previous_bounds.min_row = previous_bounds.min_row.min(row);
                previous_bounds.max_row = previous_bounds.max_row.max(row);
                previous_bounds.min_col = previous_bounds.min_col.min(col);
                previous_bounds.max_col = previous_bounds.max_col.max(col);
            }
            previous_bounds.min_row = previous_bounds.min_row.saturating_sub(halo);
            previous_bounds.max_row = previous_bounds.max_row.saturating_add(halo);
            previous_bounds.min_col = previous_bounds.min_col.saturating_sub(halo);
            previous_bounds.max_col = previous_bounds.max_col.saturating_add(halo);

            if let Some(existing) = &mut expected {
                existing.min_row = existing.min_row.min(previous_bounds.min_row);
                existing.max_row = existing.max_row.max(previous_bounds.max_row);
                existing.min_col = existing.min_col.min(previous_bounds.min_col);
                existing.max_col = existing.max_col.max(previous_bounds.max_col);
            } else {
                expected = Some(previous_bounds);
            }
        }

        prop_assert_eq!(
            compute_local_query_envelope(&centerline, &previous_cells, &frame, previous_cell_halo),
            expected,
        );
    }

    #[test]
    fn prop_ribbon_projection_is_display_metric_invariant_for_equivalent_offsets(
        display_offset in -1.25_f64..=1.25_f64,
        aspect_ratio in positive_aspect_ratio(),
    ) {
        let aspect_one = with_block_aspect_ratio(&base_frame(), 1.0);
        let aspect_two = with_block_aspect_ratio(&base_frame(), aspect_ratio);
        let centerline_one = vec![CenterSample {
            pos: RenderPoint {
                row: 10.5 - display_offset,
                col: 10.5,
            },
            tangent_row: 0.0,
            tangent_col: 1.0,
        }];
        let centerline_two = vec![CenterSample {
            pos: RenderPoint {
                row: 10.5 - display_offset / aspect_ratio,
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
            ordered_candidates(vec![
                CellCandidate {
                    state: Some(state),
                    unary_cost: 0,
                },
                CellCandidate {
                    state: None,
                    unary_cost: 10,
                },
            ]),
        )]);
        let projected_one = build_ribbon_slices(&centerline_one, &candidates, &aspect_one);
        let projected_two = build_ribbon_slices(&centerline_two, &candidates, &aspect_two);

        prop_assert_eq!(projected_one.len(), projected_two.len());
        for (slice_one, slice_two) in projected_one.iter().zip(projected_two.iter()) {
            prop_assert_eq!(slice_one.cells.len(), slice_two.cells.len());
            for (cell_one, cell_two) in slice_one.cells.iter().zip(slice_two.cells.iter()) {
                prop_assert_eq!(cell_one.coord, cell_two.coord);
                prop_assert_eq!(cell_one.normal_center_q16, cell_two.normal_center_q16);
            }
            prop_assert_eq!(slice_one.tail_u.to_bits(), slice_two.tail_u.to_bits());
            prop_assert_eq!(
                slice_one.target_width_cells.to_bits(),
                slice_two.target_width_cells.to_bits(),
            );
        }
    }
}
