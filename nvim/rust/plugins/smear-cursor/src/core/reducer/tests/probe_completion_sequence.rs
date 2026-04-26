use super::*;
use crate::test_support::proptest::stateful_config;
use crate::test_support::sparse_probe_cells;
use proptest::prelude::*;

#[derive(Clone, Debug)]
struct ProbeCompletionCase {
    cursor_color_first: bool,
    background_cell_count: usize,
    background_allowed_mask: Vec<bool>,
    cursor_color_reuse: ProbeReuse,
    cursor_color: u32,
}

fn cursor_color_reuse_strategy() -> impl Strategy<Value = ProbeReuse> {
    prop_oneof![Just(ProbeReuse::Exact), Just(ProbeReuse::Compatible),]
}

prop_compose! {
    fn probe_completion_case()(
        cursor_color_first in any::<bool>(),
        background_cell_count in prop_oneof![
            1_usize..=8_usize,
            32_usize..=96_usize,
            2047_usize..=2050_usize,
            3000_usize..=3100_usize,
            4095_usize..=4096_usize,
        ],
        cursor_color_reuse in cursor_color_reuse_strategy(),
        cursor_color in any::<u32>(),
    )(
        cursor_color_first in Just(cursor_color_first),
        background_cell_count in Just(background_cell_count),
        cursor_color_reuse in Just(cursor_color_reuse),
        cursor_color in Just(cursor_color),
        background_allowed_mask in prop::collection::vec(any::<bool>(), background_cell_count),
    ) -> ProbeCompletionCase {
        ProbeCompletionCase {
            cursor_color_first,
            background_cell_count,
            background_allowed_mask,
            cursor_color_reuse,
            cursor_color,
        }
    }
}

fn probe_sequence_scenario(
    cursor_color_first: bool,
    background_cell_count: usize,
) -> ObservationScenario {
    let ready = if cursor_color_first {
        dual_probe_ready_state()
    } else {
        background_probe_ready_state()
    };
    ObservationScenario::with_background_probe_cell_count(ready, background_cell_count)
}

fn expected_background_probe_effect(
    state: &CoreState,
    request: &PendingObservation,
    basis: &ObservationBasis,
    chunk: BackgroundProbeChunk,
) -> Effect {
    Effect::RequestProbe(RequestProbeEffect {
        observation_id: request.observation_id(),
        observation_basis: Box::new(basis.clone()),
        cursor_color_probe_generations: None,
        kind: ProbeKind::Background,
        cursor_position_policy: cursor_position_policy(state),
        buffer_perf_class: request.demand().buffer_perf_class(),
        probe_policy: expected_probe_policy(
            request.demand().kind(),
            request.demand().buffer_perf_class(),
            observation_cursor_color_fallback(state).as_ref(),
        ),
        background_chunk: Some(chunk),
        cursor_color_fallback: None,
    })
}

fn allowed_cells_for_chunk(
    chunk: &BackgroundProbeChunk,
    background_allowed_mask: &[bool],
) -> Vec<(u32, u32)> {
    chunk
        .iter_cells()
        .enumerate()
        .filter_map(|(offset, cell)| {
            background_allowed_mask
                .get(chunk.start_index().saturating_add(offset))
                .copied()
                .filter(|allowed| *allowed)
                .map(|_| {
                    (
                        u32::try_from(cell.row()).expect("probe cell row should fit into u32"),
                        u32::try_from(cell.col()).expect("probe cell col should fit into u32"),
                    )
                })
        })
        .collect()
}

proptest! {
    #![proptest_config(stateful_config())]

    #[test]
    fn prop_probe_completion_sequences_request_next_chunk_or_enter_planning(
        case in probe_completion_case(),
    ) {
        let scenario = probe_sequence_scenario(
            case.cursor_color_first,
            case.background_cell_count,
        );
        let plan_cells = sparse_probe_cells(
            scenario.basis.viewport(),
            case.background_cell_count,
        );
        let expected_cursor_color = case.cursor_color_first.then_some(case.cursor_color);
        let mut state = scenario.based.next.clone();

        if case.cursor_color_first {
            let after_cursor = reduce(
                &state,
                cursor_color_probe_report(
                    &scenario.request,
                    case.cursor_color_reuse,
                    Some(case.cursor_color),
                ),
            );
            let observation = after_cursor
                .next
                .observation()
                .expect("cursor color completion should keep observation active");
            let first_chunk = observation
                .probes()
                .background()
                .next_chunk()
                .expect("background chunk should remain pending after cursor color completion");
            let background_collecting = matches!(
                observation.probes().background(),
                BackgroundProbeState::Collecting { .. }
            );

            prop_assert_eq!(after_cursor.next.lifecycle(), Lifecycle::Observing);
            prop_assert!(after_cursor.next.pending_proposal().is_none());
            prop_assert_eq!(observation.cursor_color(), expected_cursor_color);
            prop_assert!(background_collecting);
            prop_assert_eq!(
                after_cursor.effects,
                vec![expected_background_probe_effect(
                    &after_cursor.next,
                    &scenario.request,
                    &scenario.basis,
                    first_chunk,
                )],
            );

            state = after_cursor.next;
        }

        let mut completed_cell_count = 0_usize;
        loop {
            let observation = state
                .observation()
                .expect("background probe sequence should keep an active observation");
            let chunk = observation
                .probes()
                .background()
                .next_chunk()
                .expect("background probe progress should yield the next chunk");
            let allowed_cells =
                allowed_cells_for_chunk(&chunk, &case.background_allowed_mask);
            let after_chunk = reduce(
                &state,
                background_chunk_probe_report(
                    &scenario.request,
                    &chunk,
                    scenario.basis.viewport(),
                    &allowed_cells,
                ),
            );
            completed_cell_count = completed_cell_count.saturating_add(chunk.len());

            if completed_cell_count < case.background_cell_count {
                let progressed_observation = after_chunk
                    .next
                    .observation()
                    .expect("partial background chunk should keep observation active");
                let next_chunk = progressed_observation
                    .probes()
                    .background()
                    .next_chunk()
                    .expect("partial background chunk should request the next chunk");

                prop_assert_eq!(after_chunk.next.lifecycle(), Lifecycle::Observing);
                prop_assert!(after_chunk.next.pending_proposal().is_none());
                prop_assert_eq!(next_chunk.start_index(), completed_cell_count);
                prop_assert!(progressed_observation.probes().background().batch().is_none());
                prop_assert_eq!(
                    progressed_observation.cursor_color(),
                    expected_cursor_color,
                );
                prop_assert_eq!(
                    after_chunk.effects,
                    vec![expected_background_probe_effect(
                        &after_chunk.next,
                        &scenario.request,
                        &scenario.basis,
                        next_chunk,
                    )],
                );

                state = after_chunk.next;
                continue;
            }

            prop_assert_eq!(after_chunk.next.lifecycle(), Lifecycle::Planning);
            prop_assert!(after_chunk.next.pending_proposal().is_none());
            prop_assert!(after_chunk.next.pending_plan_proposal_id().is_some());

            match after_chunk.effects.as_slice() {
                [Effect::RequestRenderPlan(effect)] => {
                    let retained_observation = after_chunk
                        .next
                        .observation()
                        .expect("planning state should retain the completed observation");
                    let observation = effect
                        .planning
                        .observation()
                        .expect("planning payload should retain the completed observation");
                    let background = observation
                        .background_probe()
                        .expect("completed observation should carry the background probe batch");

                    prop_assert_eq!(retained_observation.cursor_color(), expected_cursor_color);
                    for (index, cell) in plan_cells.iter().copied().enumerate() {
                        prop_assert_eq!(
                            background.allows_particle(cell),
                            case.background_allowed_mask[index],
                        );
                    }
                }
                other => prop_assert!(
                    false,
                    "expected render plan request after final background chunk, got {other:?}",
                ),
            }

            break;
        }
    }
}

#[test]
fn background_probe_preparation_leaves_live_runtime_unchanged_until_completion() {
    let ready = dual_probe_ready_state();
    let observing = observing_state_from_demand(&ready, ExternalDemandKind::ExternalCursor, 25);
    let request = active_request(&observing);
    let basis = observation_basis(Some(cursor(7, 8)), 26);

    let prepared = collect_observation_base(&observing, &request, basis, observation_motion());

    pretty_assert_eq!(prepared.next.runtime(), observing.runtime());
    assert!(
        prepared.next.prepared_observation_plan().is_some(),
        "background probe preparation should cache the reduced runtime motion separately",
    );
}

#[test]
fn final_background_probe_completion_reuses_the_cached_runtime_transition() {
    let ready = dual_probe_ready_state();
    let observing = observing_state_from_demand(&ready, ExternalDemandKind::ExternalCursor, 25);
    let request = active_request(&observing);
    let basis = observation_basis(Some(cursor(7, 8)), 26);
    let based = collect_observation_base(&observing, &request, basis, observation_motion());
    let prepared_plan = based
        .next
        .prepared_observation_plan()
        .cloned()
        .expect("background probe path should cache the first runtime transition");

    let scenario = ObservationScenario::with_background_plan(
        dual_probe_ready_state(),
        vec![ScreenCell::new(7, 8).expect("background probe cell")],
    );
    let mut staged = scenario.based.next.clone();
    assert!(
        staged.set_prepared_observation_plan(Some(prepared_plan.clone())),
        "manual observing scenario should accept a cached runtime transition",
    );

    let after_cursor = reduce(
        &staged,
        cursor_color_probe_report(&scenario.request, ProbeReuse::Exact, Some(0x00AB_CDEF)),
    );
    assert!(
        after_cursor.next.prepared_observation_plan().is_some(),
        "cursor-color completion should preserve the cached runtime transition",
    );

    let mutated_runtime = RuntimeState::default();
    let mutated = after_cursor.next.with_runtime(mutated_runtime);
    let mut expected_runtime = mutated.runtime().clone();
    prepared_plan.apply_to_runtime(&mut expected_runtime);
    assert_ne!(mutated.runtime(), &expected_runtime);

    let resolved = reduce(
        &mutated,
        background_probe_report(
            &scenario.request,
            scenario.basis.viewport(),
            &[(7, 8)],
            ProbeReuse::Exact,
        ),
    );

    let [Effect::RequestRenderPlan(payload)] = resolved.effects.as_slice() else {
        panic!("expected render plan request after background probe completion");
    };
    pretty_assert_eq!(resolved.next.runtime(), &expected_runtime);
    pretty_assert_eq!(
        &payload.render_decision,
        &prepared_plan.transition().render_decision,
    );
}
