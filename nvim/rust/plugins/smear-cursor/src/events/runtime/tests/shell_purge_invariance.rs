use super::ShellScratchStorageResidency;
use super::prime_shell_boundary_state;
use super::prime_shell_scratch_storage;
use super::shell_scratch_storage_residency;
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
use crate::core::state::ProbeRequestSet;
use crate::core::types::IngressSeq;
use crate::core::types::Millis;
use crate::events::runtime::FlushRedrawCapability;
use crate::events::runtime::clear_runtime_draw_context_for_test;
use crate::events::runtime::flush_redraw_capability;
use crate::events::runtime::reset_transient_shell_caches;
use crate::events::runtime::restore_draw_render_tabs;
use crate::events::runtime::set_core_state;
use crate::events::runtime::set_flush_redraw_capability;
use crate::events::runtime::with_core_transition;
use crate::host::TabHandle;
use crate::position::BufferLine;
use crate::position::CursorObservation;
use crate::position::ObservedCell;
use crate::position::ScreenCell;
use crate::position::SurfaceId;
use crate::position::ViewportBounds;
use crate::position::WindowSurfaceSnapshot;
use crate::test_support::cursor;
use pretty_assertions::assert_eq as pretty_assert_eq;
use std::collections::HashMap;

fn reduce_events_through_core_lane(initial: CoreState, events: &[Event]) -> Vec<Transition> {
    set_core_state(initial).expect("core state write should succeed");

    let mut transitions = Vec::new();
    for event in events.iter().cloned() {
        transitions.push(
            with_core_transition(|state| {
                let transition = reduce(&state, event);
                let next = transition.next.clone();
                (next, transition)
            })
            .expect("core transition should succeed"),
        );
    }
    transitions
}

#[test]
fn shell_purge_and_host_resource_reset_do_not_change_core_reducer_transitions() {
    reset_transient_shell_caches().expect("shell cache reset should succeed");
    clear_runtime_draw_context_for_test();
    set_flush_redraw_capability(FlushRedrawCapability::Unknown);

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

    prime_shell_boundary_state();
    prime_shell_scratch_storage();
    let tab_handle = TabHandle::from_raw_for_test(/*value*/ 17);
    let mut render_tabs = HashMap::new();
    render_tabs.insert(tab_handle, crate::draw::TabWindows::default());
    restore_draw_render_tabs(render_tabs);
    set_flush_redraw_capability(FlushRedrawCapability::ApiAvailable);

    pretty_assert_eq!(
        shell_scratch_storage_residency().expect("runtime access should succeed"),
        ShellScratchStorageResidency::RETAINED
    );
    pretty_assert_eq!(
        crate::events::runtime::runtime_render_tab_handles_for_test(),
        vec![tab_handle]
    );
    pretty_assert_eq!(
        flush_redraw_capability(),
        FlushRedrawCapability::ApiAvailable
    );

    let warm_transitions = reduce_events_through_core_lane(CoreState::default(), &events);

    reset_transient_shell_caches().expect("shell cache reset should succeed");
    clear_runtime_draw_context_for_test();
    set_flush_redraw_capability(FlushRedrawCapability::Unknown);

    pretty_assert_eq!(
        shell_scratch_storage_residency().expect("runtime access should succeed"),
        ShellScratchStorageResidency::RELEASED
    );
    pretty_assert_eq!(
        crate::events::runtime::runtime_render_tab_handles_for_test(),
        Vec::<TabHandle>::new()
    );
    pretty_assert_eq!(flush_redraw_capability(), FlushRedrawCapability::Unknown);

    let purged_transitions = reduce_events_through_core_lane(CoreState::default(), &events);

    pretty_assert_eq!(purged_transitions, warm_transitions);
}
