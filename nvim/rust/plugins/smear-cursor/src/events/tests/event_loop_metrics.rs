use super::super::event_loop::EventLoopState;
use super::super::event_loop::RuntimeBehaviorMetrics;

struct CounterCase {
    name: &'static str,
    repeat: usize,
    record: fn(&mut RuntimeBehaviorMetrics),
    expected: RuntimeBehaviorMetrics,
}

fn metrics_after(record: impl FnOnce(&mut RuntimeBehaviorMetrics)) -> RuntimeBehaviorMetrics {
    let mut state = EventLoopState::new();
    record(state.runtime_metrics_mut());
    state.runtime_metrics()
}

fn expected_metrics(update: impl FnOnce(&mut RuntimeBehaviorMetrics)) -> RuntimeBehaviorMetrics {
    let mut metrics = RuntimeBehaviorMetrics::new();
    update(&mut metrics);
    metrics
}

#[test]
fn event_loop_state_elapsed_autocmd_time_handles_unset_and_monotonicity() {
    let mut state = EventLoopState::new();
    assert!(
        state
            .elapsed_ms_since_last_autocmd_event(10.0)
            .is_infinite()
    );

    state.note_autocmd_event(20.0);
    assert_eq!(state.elapsed_ms_since_last_autocmd_event(25.0), 5.0);
    assert_eq!(state.elapsed_ms_since_last_autocmd_event(19.0), 0.0);

    state.clear_autocmd_event_timestamp();
    assert!(
        state
            .elapsed_ms_since_last_autocmd_event(30.0)
            .is_infinite()
    );
}

#[test]
fn event_loop_state_records_counter_updates() {
    let cases = [
        CounterCase {
            name: "received_ingress",
            repeat: 2,
            record: RuntimeBehaviorMetrics::record_ingress_received,
            expected: expected_metrics(|metrics| metrics.ingress_received = 2),
        },
        CounterCase {
            name: "applied_ingress",
            repeat: 1,
            record: RuntimeBehaviorMetrics::record_ingress_applied,
            expected: expected_metrics(|metrics| metrics.ingress_applied = 1),
        },
        CounterCase {
            name: "dropped_ingress",
            repeat: 1,
            record: RuntimeBehaviorMetrics::record_ingress_dropped,
            expected: expected_metrics(|metrics| metrics.ingress_dropped = 1),
        },
        CounterCase {
            name: "coalesced_ingress",
            repeat: 1,
            record: RuntimeBehaviorMetrics::record_ingress_coalesced,
            expected: expected_metrics(|metrics| metrics.ingress_coalesced = 1),
        },
        CounterCase {
            name: "observation_request_executed",
            repeat: 1,
            record: RuntimeBehaviorMetrics::record_observation_request_executed,
            expected: expected_metrics(|metrics| metrics.observation_requests_executed = 1),
        },
        CounterCase {
            name: "degraded_draw_application",
            repeat: 1,
            record: RuntimeBehaviorMetrics::record_degraded_draw_application,
            expected: expected_metrics(|metrics| metrics.degraded_draw_applications = 1),
        },
        CounterCase {
            name: "stale_token_event",
            repeat: 1,
            record: RuntimeBehaviorMetrics::record_stale_token_event,
            expected: expected_metrics(|metrics| metrics.stale_token_events = 1),
        },
        CounterCase {
            name: "buffer_metadata_read",
            repeat: 2,
            record: RuntimeBehaviorMetrics::record_buffer_metadata_read,
            expected: expected_metrics(|metrics| {
                metrics.validation_reads.buffer_metadata_reads = 2;
            }),
        },
        CounterCase {
            name: "current_buffer_changedtick_read",
            repeat: 3,
            record: RuntimeBehaviorMetrics::record_current_buffer_changedtick_read,
            expected: expected_metrics(|metrics| {
                metrics.validation_reads.current_buffer_changedtick_reads = 3;
            }),
        },
        CounterCase {
            name: "editor_bounds_read",
            repeat: 2,
            record: RuntimeBehaviorMetrics::record_editor_bounds_read,
            expected: expected_metrics(|metrics| {
                metrics.validation_reads.editor_bounds_reads = 2;
            }),
        },
        CounterCase {
            name: "command_row_read",
            repeat: 4,
            record: RuntimeBehaviorMetrics::record_command_row_read,
            expected: expected_metrics(|metrics| {
                metrics.validation_reads.command_row_reads = 4;
            }),
        },
    ];

    for case in cases {
        let metrics = metrics_after(|metrics| {
            for _ in 0..case.repeat {
                (case.record)(metrics);
            }
        });
        assert_eq!(metrics, case.expected, "case: {}", case.name);
    }
}
