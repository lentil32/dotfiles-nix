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
