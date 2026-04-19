use super::*;
use crate::test_support::proptest::approx_eq_f64;
use crate::test_support::proptest::positive_aspect_ratio;
use crate::test_support::proptest::pure_config;
use proptest::collection::vec;
use proptest::prelude::*;

fn full_width_tile() -> latent_field::MicroTile {
    tile_for_column_span(0, latent_field::MICRO_W - 1, 0x0FFF)
}

fn straight_vertical_centerline(len: usize) -> Vec<CenterSample> {
    (0..len)
        .map(|index| CenterSample {
            pos: RenderPoint {
                row: 10.5 + index as f64,
                col: 10.5,
            },
            tangent_row: 1.0,
            tangent_col: 0.0,
        })
        .collect()
}

fn compiled_support(
    row_count: usize,
    support_cell_count: usize,
) -> BTreeMap<(i64, i64), latent_field::CompiledCell> {
    let tile = full_width_tile();
    let mut compiled = BTreeMap::new();
    for row_offset in 0..row_count {
        for col_offset in 0..support_cell_count {
            compiled.insert(
                (10 + row_offset as i64, 10 + col_offset as i64),
                latent_field::CompiledCell {
                    tile,
                    age: AgeMoment::default(),
                },
            );
        }
    }
    compiled
}

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_cell_row_index_matches_linear_scan_in_requested_bounds(
        entries in vec((8_i64..=12_i64, 8_i64..=24_i64, any::<u16>()), 0..=48),
        row_a in 8_i64..=12_i64,
        row_b in 8_i64..=12_i64,
        col_a in 8_i64..=24_i64,
        col_b in 8_i64..=24_i64,
    ) {
        let cells = entries
            .into_iter()
            .map(|(row, col, value)| ((row, col), value))
            .collect::<BTreeMap<_, _>>();
        let bounds = SliceSearchBounds {
            min_row: row_a.min(row_b),
            max_row: row_a.max(row_b),
            min_col: col_a.min(col_b),
            max_col: col_a.max(col_b),
        };
        let expected = cells
            .iter()
            .filter_map(|(&coord, _)| bounds.contains(coord).then_some(coord))
            .collect::<Vec<_>>();
        let mut scratch = CellRowIndexScratch::default();
        let index = CellRowIndex::build(&cells, &mut scratch);
        let mut visited = Vec::<(i64, i64)>::new();

        index.for_each_in_bounds(bounds, |coord, _| visited.push(coord));

        prop_assert_eq!(visited, expected);
    }

    #[test]
    fn prop_comet_taper_target_is_monotonic_from_head_to_tip(
        head_width in 1.0_f64..=6.0_f64,
        sample_count in 2_usize..=24_usize,
    ) {
        let widths = (0..sample_count)
            .map(|index| {
                let u = index as f64 / (sample_count - 1) as f64;
                comet_target_width_cells(head_width, u)
            })
            .collect::<Vec<_>>();

        prop_assert!(approx_eq_f64(widths[0], head_width, 1.0e-9));
        for pair in widths.windows(2) {
            prop_assert!(
                pair[0] + 1.0e-9 >= pair[1],
                "taper should be monotonic: {widths:?}",
            );
        }
        prop_assert!(
            widths
                .last()
                .is_some_and(|tip| *tip >= COMET_MIN_RESOLVABLE_WIDTH)
        );
    }

    #[test]
    fn prop_slice_target_width_tracks_compiled_support_width(
        aspect_ratio in positive_aspect_ratio(),
    ) {
        let frame = with_block_aspect_ratio(&with_trail_thickness(&base_frame(), 1.0), aspect_ratio);
        let centerline = straight_vertical_centerline(1);
        let narrow_compiled = compiled_support(1, 1);
        let wide_compiled = compiled_support(1, 2);
        let narrow_candidates = build_cell_candidates(
            &narrow_compiled,
            &BTreeMap::new(),
            frame.color_levels,
            0.0,
            5,
        );
        let wide_candidates =
            build_cell_candidates(&wide_compiled, &BTreeMap::new(), frame.color_levels, 0.0, 5);
        let narrow_slices =
            build_ribbon_slices_with_compiled(&centerline, &narrow_compiled, &narrow_candidates, &frame);
        let wide_slices =
            build_ribbon_slices_with_compiled(&centerline, &wide_compiled, &wide_candidates, &frame);

        prop_assert_eq!(narrow_slices.len(), 1);
        prop_assert_eq!(wide_slices.len(), 1);
        prop_assert!(
            wide_slices[0].target_width_cells > narrow_slices[0].target_width_cells,
            "wider latent support should produce a wider target: wide={} narrow={}",
            wide_slices[0].target_width_cells,
            narrow_slices[0].target_width_cells,
        );
    }

    #[test]
    fn prop_slice_taper_targets_stay_stable_across_aspect_ratios_when_support_width_is_unchanged(
        support_cell_count in 1_usize..=3_usize,
        aspect_one in positive_aspect_ratio(),
        aspect_two in positive_aspect_ratio(),
    ) {
        let frame = with_trail_thickness(&base_frame(), 1.0);
        let centerline = straight_vertical_centerline(1);
        let compiled = compiled_support(1, support_cell_count);
        let candidates = build_cell_candidates(&compiled, &BTreeMap::new(), frame.color_levels, 0.0, 5);
        let aspect_one_frame = with_block_aspect_ratio(&frame, aspect_one);
        let aspect_two_frame = with_block_aspect_ratio(&frame, aspect_two);
        let aspect_one_slices =
            build_ribbon_slices_with_compiled(&centerline, &compiled, &candidates, &aspect_one_frame);
        let aspect_two_slices =
            build_ribbon_slices_with_compiled(&centerline, &compiled, &candidates, &aspect_two_frame);

        prop_assert_eq!(aspect_one_slices.len(), 1);
        prop_assert_eq!(aspect_two_slices.len(), 1);
        prop_assert!(
            approx_eq_f64(
                aspect_one_slices[0].target_width_cells,
                aspect_two_slices[0].target_width_cells,
                1.0e-6,
            ),
            "taper targets should remain stable when aspect ratio changes without changing cross-track support width: aspect1={} aspect2={}",
            aspect_one_slices[0].target_width_cells,
            aspect_two_slices[0].target_width_cells,
        );
    }
}
