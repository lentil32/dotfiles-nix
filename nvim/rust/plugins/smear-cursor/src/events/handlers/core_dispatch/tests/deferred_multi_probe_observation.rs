use super::*;
use pretty_assertions::assert_eq;

fn setup_multi_probe_ingress() -> (
    CoreDispatchTestContext,
    ObservationRequest,
    ObservationBasis,
) {
    let scope = CoreDispatchTestContext::new();
    scope.set_core_state(ready_state_with_cursor_and_background_probes());
    let request = scope.dispatch_external_cursor_ingress_to_queue(25);
    let basis = observation_basis(&request, Some(cursor(7, 8)), 26);
    (scope, request, basis)
}

fn setup_after_observation_base_edge() -> (
    CoreDispatchTestContext,
    ObservationRequest,
    ObservationBasis,
    RecordingExecutor,
) {
    let (scope, request, basis) = setup_multi_probe_ingress();
    let mut executor = RecordingExecutor::default();
    executor
        .planned_follow_ups
        .push_back(vec![observation_base_collected(&request, basis.clone())]);
    let _ = drain_next_edge(&mut executor);
    install_background_probe_plan(&request, &basis);
    (scope, request, basis, executor)
}

fn setup_after_cursor_color_probe_edge() -> (
    CoreDispatchTestContext,
    ObservationRequest,
    ObservationBasis,
    BackgroundProbeChunk,
    RecordingExecutor,
) {
    let (scope, request, basis, mut executor) = setup_after_observation_base_edge();
    executor
        .planned_follow_ups
        .push_back(vec![compatible_probe_report(&request)]);
    let _ = drain_next_edge(&mut executor);
    let first_background_chunk = current_core_state()
        .observation()
        .and_then(|observation| observation.background_progress())
        .and_then(crate::core::state::BackgroundProbeProgress::next_chunk)
        .expect("first background chunk");
    (scope, request, basis, first_background_chunk, executor)
}

#[test]
fn observation_base_edge_queues_only_the_cursor_color_probe() {
    let (_scope, _request, _basis, executor) = setup_after_observation_base_edge();

    assert!(matches!(
        executor.executed_effects.as_slice(),
        [Effect::RequestObservationBase(_), Effect::ScheduleTimer(_)]
    ));
    assert_eq!(
        queued_work_count(),
        1,
        "only one probe batch should be queued"
    );
    assert!(matches!(
        queued_front_work_item(),
        Some(ScheduledWorkUnit::EffectBatch(ref effects))
            if only_cursor_color_probe_request(effects)
    ));
}

#[test]
fn cursor_color_probe_edge_queues_the_first_background_chunk() {
    let (_scope, _request, _basis, first_background_chunk, executor) =
        setup_after_cursor_color_probe_edge();

    assert!(
        executor.executed_effects.iter().any(|effect| matches!(
            effect,
            Effect::RequestProbe(payload) if payload.kind == ProbeKind::CursorColor
        )),
        "cursor-color edge should execute the cursor-color probe request",
    );
    assert_eq!(queued_work_count(), 1);
    assert!(matches!(
        queued_front_work_item(),
        Some(ScheduledWorkUnit::EffectBatch(ref effects))
            if only_background_probe_request_for_chunk(effects, &first_background_chunk)
    ));
}

#[test]
fn background_chunk_edge_queues_the_next_background_chunk() {
    let (_scope, request, basis, first_background_chunk, mut executor) =
        setup_after_cursor_color_probe_edge();
    executor
        .planned_follow_ups
        .push_back(vec![background_chunk_probe_report(
            &request,
            &first_background_chunk,
            basis.viewport(),
        )]);

    let has_more_items = drain_next_edge(&mut executor);
    let second_background_chunk = current_core_state()
        .observation()
        .and_then(|observation| observation.background_progress())
        .and_then(crate::core::state::BackgroundProbeProgress::next_chunk)
        .expect("second background chunk");

    assert!(
        has_more_items,
        "background chunk completion should queue the next chunk"
    );
    assert!(
        executor.executed_effects.iter().any(|effect| matches!(
            effect,
            Effect::RequestProbe(payload)
                if payload.kind == ProbeKind::Background
                    && payload.background_chunk.as_ref() == Some(&first_background_chunk)
        )),
        "background-chunk edge should execute the current background probe request",
    );
    assert_eq!(queued_work_count(), 1);
    assert!(matches!(
        queued_front_work_item(),
        Some(ScheduledWorkUnit::EffectBatch(ref effects))
            if only_background_probe_request_for_chunk(effects, &second_background_chunk)
    ));
}

#[test]
fn final_background_edge_transitions_the_runtime_to_planning() {
    let (_scope, request, basis, first_background_chunk, mut executor) =
        setup_after_cursor_color_probe_edge();
    executor
        .planned_follow_ups
        .push_back(vec![background_chunk_probe_report(
            &request,
            &first_background_chunk,
            basis.viewport(),
        )]);
    let _ = drain_next_edge(&mut executor);
    executor
        .planned_follow_ups
        .push_back(vec![background_probe_report(&request, basis.viewport())]);

    let _ = drain_next_edge(&mut executor);
    let after_completion = current_core_state();

    assert_eq!(after_completion.lifecycle(), Lifecycle::Planning);
    assert!(after_completion.pending_proposal().is_none());
    assert!(after_completion.pending_plan_proposal_id().is_some());
}

#[test]
fn final_background_edge_queues_render_plan_work_for_a_later_edge() {
    let (_scope, request, basis, first_background_chunk, mut executor) =
        setup_after_cursor_color_probe_edge();
    executor
        .planned_follow_ups
        .push_back(vec![background_chunk_probe_report(
            &request,
            &first_background_chunk,
            basis.viewport(),
        )]);
    let _ = drain_next_edge(&mut executor);
    executor
        .planned_follow_ups
        .push_back(vec![background_probe_report(&request, basis.viewport())]);

    let has_more_items = drain_next_edge(&mut executor);

    assert!(
        has_more_items,
        "planning work should remain deferred to a later edge"
    );
    assert_eq!(queued_work_count(), 1, "planning work should remain queued");
    assert!(matches!(
        queued_front_work_item(),
        Some(ScheduledWorkUnit::EffectBatch(ref effects))
            if contains_render_plan_request(effects)
    ));
}
