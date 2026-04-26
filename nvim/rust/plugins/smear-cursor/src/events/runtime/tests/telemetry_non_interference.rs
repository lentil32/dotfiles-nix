use crate::core::effect::Effect;
use crate::core::event::Event;
use crate::core::event::ExternalDemandQueuedEvent;
use crate::core::event::InitializeEvent;
use crate::core::event::ObservationBaseCollectedEvent;
use crate::core::reducer::Transition;
use crate::core::reducer::reduce;
use crate::core::state::BufferPerfClass;
use crate::core::state::CoreState;
use crate::core::state::ExternalDemand;
use crate::core::state::ExternalDemandKind;
use crate::core::state::ObservationBasis;
use crate::core::state::ObservationMotion;
use crate::core::state::PendingObservation;
use crate::core::state::ProbeKind;
use crate::core::state::ProbeRequestSet;
use crate::core::state::RenderThermalState;
use crate::core::types::IngressSeq;
use crate::core::types::Lifecycle;
use crate::core::types::Millis;
use crate::events::runtime::record_cursor_callback_duration;
use crate::events::runtime::record_delayed_ingress_pending_update_count;
use crate::events::runtime::record_ingress_coalesced_count;
use crate::events::runtime::record_ingress_received;
use crate::events::runtime::record_post_burst_convergence;
use crate::events::runtime::record_probe_refresh_budget_exhausted_count;
use crate::events::runtime::record_probe_refresh_retried_count;
use crate::events::runtime::record_scheduled_drain_items;
use crate::events::runtime::record_scheduled_drain_items_for_thermal;
use crate::events::runtime::record_scheduled_drain_reschedule;
use crate::events::runtime::record_scheduled_drain_reschedule_for_thermal;
use crate::events::runtime::record_scheduled_queue_depth;
use crate::events::runtime::record_scheduled_queue_depth_for_thermal;
use crate::events::runtime::record_stale_token_event_count;
use crate::events::runtime::set_core_state;
use crate::events::runtime::with_core_transition;
use crate::events::runtime::with_event_loop_state_for_test;
use crate::position::BufferLine;
use crate::position::CursorObservation;
use crate::position::ObservedCell;
use crate::position::ScreenCell;
use crate::position::SurfaceId;
use crate::position::ViewportBounds;
use crate::position::WindowSurfaceSnapshot;
use crate::test_support::cursor;
use pretty_assertions::assert_eq as pretty_assert_eq;

#[derive(Debug, Clone, PartialEq)]
struct ReducerRunObservation {
    transitions: Vec<Transition>,
    lifecycle_decisions: Vec<Lifecycle>,
    emitted_effects: Vec<Vec<Effect>>,
}

#[derive(Debug, Clone, Copy)]
enum TelemetrySamplingMode {
    Recorded,
    Dropped,
}

impl TelemetrySamplingMode {
    fn record_before_transition(self) {
        match self {
            Self::Recorded => record_representative_telemetry_samples(),
            Self::Dropped => record_representative_telemetry_samples_while_lane_is_borrowed(),
        }
    }
}

fn reduce_events_through_core_lane(
    initial: CoreState,
    events: &[Event],
    telemetry_mode: TelemetrySamplingMode,
) -> ReducerRunObservation {
    crate::events::event_loop::reset_for_test();
    set_core_state(initial).expect("core state write should succeed");

    let mut transitions = Vec::new();
    for event in events.iter().cloned() {
        telemetry_mode.record_before_transition();
        transitions.push(
            with_core_transition(|state| {
                let transition = reduce(&state, event);
                let next = transition.next.clone();
                (next, transition)
            })
            .expect("core transition should succeed"),
        );
    }
    let lifecycle_decisions = transitions
        .iter()
        .map(|transition| transition.next.lifecycle())
        .collect();
    let emitted_effects = transitions
        .iter()
        .map(|transition| transition.effects.clone())
        .collect();
    ReducerRunObservation {
        transitions,
        lifecycle_decisions,
        emitted_effects,
    }
}

fn record_representative_telemetry_samples() {
    record_ingress_received();
    record_ingress_coalesced_count(/*count*/ 2);
    record_delayed_ingress_pending_update_count(/*count*/ 1);
    record_stale_token_event_count(/*count*/ 3);
    record_scheduled_queue_depth(/*depth*/ 5);
    record_scheduled_queue_depth_for_thermal(RenderThermalState::Hot, /*depth*/ 8);
    record_scheduled_drain_items(/*drained_items*/ 4);
    record_scheduled_drain_items_for_thermal(RenderThermalState::Cooling, /*drained_items*/ 6);
    record_scheduled_drain_reschedule();
    record_scheduled_drain_reschedule_for_thermal(RenderThermalState::Cold);
    record_post_burst_convergence(Millis::new(/*value*/ 31), Millis::new(/*value*/ 37));
    record_probe_refresh_retried_count(ProbeKind::CursorColor, /*count*/ 1);
    record_probe_refresh_budget_exhausted_count(ProbeKind::Background, /*count*/ 1);
    record_cursor_callback_duration(/*buffer_handle*/ None, /*duration_ms*/ 12.0);
}

fn record_representative_telemetry_samples_while_lane_is_borrowed() {
    with_event_loop_state_for_test(|state| {
        let before = state.diagnostics_snapshot();
        record_representative_telemetry_samples();
        pretty_assert_eq!(state.diagnostics_snapshot(), before);
    });
}

#[test]
fn dropped_telemetry_samples_do_not_change_core_reducer_observations() {
    let request = PendingObservation::new(
        ExternalDemand::new(
            IngressSeq::new(/*value*/ 1),
            ExternalDemandKind::ExternalCursor,
            Millis::new(/*value*/ 21),
            BufferPerfClass::Full,
        ),
        ProbeRequestSet::default(),
    );
    let buffer_revision = Some(0);
    let events = vec![
        Event::Initialize(InitializeEvent {
            observed_at: Millis::new(/*value*/ 11),
        }),
        Event::ExternalDemandQueued(ExternalDemandQueuedEvent {
            kind: ExternalDemandKind::ExternalCursor,
            observed_at: Millis::new(/*value*/ 21),
            buffer_perf_class: BufferPerfClass::Full,
            ingress_cursor_presentation: None,
            ingress_observation_surface: None,
        }),
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            observation_id: request.observation_id(),
            basis: ObservationBasis::new(
                Millis::new(/*value*/ 22),
                "n".to_string(),
                WindowSurfaceSnapshot::new(
                    SurfaceId::new(/*window_handle*/ 11, /*buffer_handle*/ 22)
                        .expect("positive handles"),
                    BufferLine::new(/*line*/ 3).expect("positive top buffer line"),
                    /*left_col0*/ 0,
                    /*text_offset0*/ 0,
                    ScreenCell::new(/*row*/ 1, /*col*/ 1).expect("one-based window origin"),
                    ViewportBounds::new(/*max_row*/ 40, /*max_col*/ 120)
                        .expect("positive window size"),
                ),
                CursorObservation::new(
                    BufferLine::new(/*line*/ 4).expect("positive buffer line"),
                    ObservedCell::Exact(cursor(/*row*/ 7, /*col*/ 8)),
                ),
                ViewportBounds::new(/*max_row*/ 40, /*max_col*/ 120)
                    .expect("positive viewport bounds"),
            )
            .with_buffer_revision(buffer_revision),
            cursor_color_probe_generations: None,
            motion: ObservationMotion::new(/*scroll_shift*/ None),
        }),
    ];

    let recorded = reduce_events_through_core_lane(
        CoreState::default(),
        &events,
        TelemetrySamplingMode::Recorded,
    );
    let dropped = reduce_events_through_core_lane(
        CoreState::default(),
        &events,
        TelemetrySamplingMode::Dropped,
    );
    crate::events::event_loop::reset_for_test();

    pretty_assert_eq!(dropped, recorded);
}
