use super::*;
use crate::position::ObservedCell;
use crate::position::RenderPoint;
use crate::position::ScreenCell;
use pretty_assertions::assert_eq;

fn ready_state_with_initialized_runtime(
    runtime_target: ScreenCell,
    latest_exact_cursor_cell: Option<ScreenCell>,
) -> CoreState {
    let mut runtime = ready_state().runtime().clone();
    runtime.initialize_cursor(
        RenderPoint {
            row: runtime_target.row() as f64,
            col: runtime_target.col() as f64,
        },
        CursorShape::block(),
        7,
        &TrackedCursor::fixture(11, 22, 3, 4)
            .with_window_origin(1, 1)
            .with_window_dimensions(120, 40),
    );
    runtime.record_observed_mode(/*current_is_cmdline*/ false);

    ready_state()
        .with_runtime(runtime)
        .with_latest_exact_cursor_cell(latest_exact_cursor_cell)
}

fn collect_external_cursor_observation(
    ready: &CoreState,
    observed_cell: ObservedCell,
    observed_at: u64,
) -> Transition {
    let observing = reduce(
        ready,
        external_demand_event(ExternalDemandKind::ExternalCursor, observed_at - 1),
    )
    .next;
    let request = active_request(&observing);

    collect_observation_base(
        &observing,
        &request,
        observation_basis_with_observed_cell(observed_cell, observed_at, "n"),
        observation_motion(),
    )
}

#[test]
fn queued_external_cursor_demand_does_not_shadow_the_runtime_target() {
    let ready = ready_state_with_initialized_runtime(cursor(10, 10), Some(cursor(10, 10)));
    let baseline_target = ready.runtime().target_position();

    let transition = reduce(
        &ready,
        external_demand_event(ExternalDemandKind::ExternalCursor, 100),
    );

    assert_eq!(transition.next.runtime().target_position(), baseline_target);
    assert_eq!(
        transition.next.latest_exact_cursor_cell(),
        Some(cursor(10, 10))
    );
    assert_eq!(
        transition
            .next
            .pending_observation()
            .map(|request| request.demand().kind()),
        Some(ExternalDemandKind::ExternalCursor)
    );
}

#[test]
fn unavailable_observation_without_an_anchor_keeps_the_runtime_target() {
    let runtime_target = cursor(10, 10);
    let ready = ready_state_with_initialized_runtime(runtime_target, None);

    let transition = collect_external_cursor_observation(&ready, ObservedCell::Unavailable, 101);

    let [Effect::RequestRenderPlan(_)] = transition.effects.as_slice() else {
        panic!("expected render plan request after unavailable observation");
    };
    assert_eq!(transition.next.lifecycle(), Lifecycle::Planning);
    assert_eq!(
        ScreenCell::from_rounded_point(transition.next.runtime().target_position()),
        Some(runtime_target)
    );
    assert_eq!(transition.next.latest_exact_cursor_cell(), None);
}
