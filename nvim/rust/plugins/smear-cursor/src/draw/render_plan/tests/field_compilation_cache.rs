use super::*;
use crate::test_support::proptest::pure_config;
use proptest::prelude::*;

proptest! {
    #![proptest_config(pure_config())]

    #[test]
    fn prop_compile_render_frame_falls_back_to_reference_when_previous_halo_exceeds_budget(
        far_span in LOCAL_QUERY_ENVELOPE_FAST_PATH_MAX_AREA_CELLS as i64
            ..= LOCAL_QUERY_ENVELOPE_FAST_PATH_MAX_AREA_CELLS as i64 + 512_i64,
        left_level in 1_u8..=16_u8,
        right_level in 1_u8..=16_u8,
    ) {
        let frame = base_frame();
        let state = PlannerState {
            previous_cells: Arc::new(BTreeMap::from([
                ((8_i64, 8_i64), highlight_state(u32::from(left_level))),
                ((8_i64, 8_i64 + far_span), highlight_state(u32::from(right_level))),
            ])),
            ..PlannerState::default()
        };

        let compiled = compile_render_frame(&frame, state.clone());
        let reference = compile_render_frame_reference(&frame, state);
        let envelope = compute_local_query_envelope(
            &compiled.next_state.decode_scratch.centerline,
            &compiled.next_state.previous_cells,
            &frame,
            PREVIOUS_CELL_HALO_CELLS,
        )
        .expect("oversized previous-cell halos should still produce a finite envelope");

        prop_assert!(
            query_envelope_area_cells(envelope) > LOCAL_QUERY_ENVELOPE_FAST_PATH_MAX_AREA_CELLS
        );
        prop_assert_eq!(compiled.query_bounds, None);
        prop_assert!(matches!(compiled.compiled.as_ref(), CompiledField::Reference(_)));
        prop_assert_eq!(compiled.compiled.to_btree_map(), reference);
    }
}
