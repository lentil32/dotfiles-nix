use super::*;
use crate::core::effect::RequestProbeEffect;
use pretty_assertions::assert_eq;

fn setup_cursor_probe_ingress() -> (CoreDispatchTestContext, ObservationRequest) {
    let scope = CoreDispatchTestContext::new();
    scope.set_core_state(ready_state_with_cursor_color_probe());
    let request = scope.dispatch_external_cursor_ingress_to_queue(25);
    (scope, request)
}

#[test]
fn ingress_dispatch_queues_one_observation_base_batch() {
    let (_scope, _request) = setup_cursor_probe_ingress();
    let after_ingress = current_core_state();

    assert_eq!(after_ingress.lifecycle(), Lifecycle::Observing);
    assert!(after_ingress.observation().is_none());
    assert!(after_ingress.pending_proposal().is_none());
    assert_eq!(
        queued_work_count(),
        1,
        "ingress should queue one effect batch"
    );
    assert!(matches!(
        queued_front_work_item(),
        Some(ScheduledWorkUnit::EffectBatch(ref effects))
            if contains_observation_base_request(effects)
    ));
}

#[test]
fn observation_base_edge_executes_the_same_wave_cursor_probe() {
    let (_scope, request) = setup_cursor_probe_ingress();
    let mut executor = RecordingExecutor::default();
    executor
        .planned_follow_ups
        .push_back(vec![observation_base_collected(
            &request,
            observation_basis(&request, Some(cursor(7, 8)), 26),
        )]);
    executor.planned_follow_ups.push_back(Vec::new());
    executor
        .planned_follow_ups
        .push_back(vec![compatible_probe_report(&request)]);

    let has_more_items = drain_next_edge(&mut executor);

    assert!(
        has_more_items,
        "same-wave cursor probe completion should still leave planning/apply work for a later edge"
    );
    assert!(matches!(
        executor.executed_effects.as_slice(),
        [
            Effect::RequestObservationBase(_),
            Effect::ScheduleTimer(_),
            Effect::RequestProbe(RequestProbeEffect {
                kind: ProbeKind::CursorColor,
                ..
            })
        ]
    ));
}

#[test]
fn observation_base_edge_updates_the_retained_observation_from_same_wave_probe() {
    let (_scope, request) = setup_cursor_probe_ingress();
    let mut executor = RecordingExecutor::default();
    executor
        .planned_follow_ups
        .push_back(vec![observation_base_collected(
            &request,
            observation_basis(&request, Some(cursor(7, 8)), 26),
        )]);
    executor.planned_follow_ups.push_back(Vec::new());
    executor
        .planned_follow_ups
        .push_back(vec![compatible_probe_report(&request)]);

    let _ = drain_next_edge(&mut executor);

    assert_eq!(
        current_core_state()
            .observation()
            .and_then(crate::core::state::ObservationSnapshot::cursor_color),
        Some(0x00AB_CDEF)
    );
}

fn setup_after_same_wave_cursor_probe() -> (
    CoreDispatchTestContext,
    ObservationRequest,
    RecordingExecutor,
) {
    let (scope, request) = setup_cursor_probe_ingress();
    let mut executor = RecordingExecutor::default();
    executor
        .planned_follow_ups
        .push_back(vec![observation_base_collected(
            &request,
            observation_basis(&request, Some(cursor(7, 8)), 26),
        )]);
    executor.planned_follow_ups.push_back(Vec::new());
    executor
        .planned_follow_ups
        .push_back(vec![compatible_probe_report(&request)]);
    let _ = drain_next_edge(&mut executor);
    (scope, request, executor)
}

#[test]
fn same_wave_cursor_probe_keeps_apply_work_deferred() {
    let (_scope, _request, executor) = setup_after_same_wave_cursor_probe();

    assert!(
        !executor.executed_effects.iter().any(is_apply_proposal),
        "apply work must remain deferred after the same-wave probe finishes because planning still runs first"
    );
}

#[test]
fn same_wave_cursor_probe_leaves_only_non_probe_follow_up_work_queued() {
    let (_scope, _request, _executor) = setup_after_same_wave_cursor_probe();
    let has_more_items = queued_work_count() > 0;
    let queued_follow_up = queued_front_work_item();

    assert!(
        if has_more_items {
            matches!(
                queued_follow_up,
                Some(ScheduledWorkUnit::EffectBatch(ref effects))
                    if !contains_probe_request(effects)
                        && (contains_render_plan_request(effects)
                            || effects.iter().any(is_apply_proposal))
            )
        } else {
            queued_follow_up.is_none()
        },
        "same-wave probe completion should either queue planning/apply work or finish without extra shell work"
    );
}
