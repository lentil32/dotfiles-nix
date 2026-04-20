use super::*;
use crate::core::effect::RequestProbeEffect;
use pretty_assertions::assert_eq;

fn setup_cursor_probe_ingress() -> (CoreDispatchTestContext, PendingObservation) {
    let scope = CoreDispatchTestContext::new();
    scope.set_core_state(ready_state_with_cursor_color_probe());
    let request = scope.dispatch_external_cursor_ingress_to_queue(25);
    (scope, request)
}

fn same_wave_cursor_probe_executor(request: &PendingObservation) -> RecordingExecutor {
    let mut executor = RecordingExecutor::default();
    executor
        .planned_follow_ups
        .push_back(vec![observation_base_collected(
            request,
            observation_basis(Some(cursor(7, 8)), 26),
        )]);
    executor.planned_follow_ups.push_back(Vec::new());
    executor
        .planned_follow_ups
        .push_back(vec![compatible_probe_report(request)]);
    executor
}

fn run_same_wave_cursor_probe_edge() -> (
    CoreDispatchTestContext,
    PendingObservation,
    RecordingExecutor,
    bool,
) {
    let (scope, request) = setup_cursor_probe_ingress();
    let mut executor = same_wave_cursor_probe_executor(&request);
    let has_more_items = drain_next_edge(&mut executor);
    (scope, request, executor, has_more_items)
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
fn observation_base_edge_executes_the_same_wave_cursor_probe_and_updates_observation() {
    let (_scope, _request, executor, has_more_items) = run_same_wave_cursor_probe_edge();

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
    assert_eq!(
        current_core_state()
            .observation()
            .and_then(crate::core::state::ObservationSnapshot::cursor_color),
        Some(0x00AB_CDEF)
    );
}

#[test]
fn same_wave_cursor_probe_defers_apply_work_and_leaves_only_non_probe_follow_up_work_queued() {
    let (_scope, _request, executor, has_more_items) = run_same_wave_cursor_probe_edge();
    let queued_follow_up = queued_front_work_item();

    assert!(
        !executor.executed_effects.iter().any(is_apply_proposal),
        "apply work must remain deferred after the same-wave probe finishes because planning still runs first"
    );
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
