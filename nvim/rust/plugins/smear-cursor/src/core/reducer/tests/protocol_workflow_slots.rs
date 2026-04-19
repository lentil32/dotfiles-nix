use super::*;
use crate::core::state::ProtocolPhaseKind;
use crate::test_support::cursor;

fn assert_phase_views(
    state: &CoreState,
    expected_lifecycle: Lifecycle,
    expected_kind: ProtocolPhaseKind,
) {
    pretty_assert_eq!(state.lifecycle(), expected_lifecycle);
    pretty_assert_eq!(state.protocol().lifecycle(), expected_lifecycle);
    pretty_assert_eq!(state.protocol().phase_kind(), expected_kind);
}

fn assert_observation_views(
    state: &CoreState,
    expected_pending: Option<&PendingObservation>,
    expected_active: Option<&ObservationSnapshot>,
    expected_phase_owned: Option<&ObservationSnapshot>,
    expected_retained: Option<&ObservationSnapshot>,
) {
    let expected_active_demand = expected_pending
        .map(PendingObservation::demand)
        .or_else(|| expected_active.map(ObservationSnapshot::demand));

    pretty_assert_eq!(state.active_demand(), expected_active_demand);
    pretty_assert_eq!(state.pending_observation(), expected_pending);
    pretty_assert_eq!(state.observation(), expected_active);
    pretty_assert_eq!(state.phase_observation(), expected_phase_owned);
    pretty_assert_eq!(state.retained_observation(), expected_retained);
}

#[test]
fn idle_and_primed_states_derive_workflow_views_from_protocol_phase() {
    let idle = CoreState::default();

    assert_phase_views(&idle, Lifecycle::Idle, ProtocolPhaseKind::Idle);
    assert_observation_views(&idle, None, None, None, None);
    assert!(idle.needs_initialize());

    let primed = reduce(
        &idle,
        Event::Initialize(InitializeEvent {
            observed_at: Millis::new(11),
        }),
    )
    .next;

    assert_phase_views(&primed, Lifecycle::Primed, ProtocolPhaseKind::Primed);
    assert_observation_views(&primed, None, None, None, None);
    assert!(!primed.needs_initialize());
}

#[test]
fn enter_observing_request_demotes_active_observation_into_the_collecting_phase() {
    let observation = observation_snapshot(cursor(7, 8));
    let request = observation_request(3, ExternalDemandKind::ExternalCursor, 20);
    let observing = ready_state()
        .with_ready_observation(observation.clone())
        .expect("primed state should accept a ready observation")
        .enter_observing_request(request.clone())
        .expect("ready state should stage a collecting observation request");

    assert_phase_views(
        &observing,
        Lifecycle::Observing,
        ProtocolPhaseKind::Collecting,
    );
    assert_observation_views(
        &observing,
        Some(&request),
        None,
        Some(&observation),
        Some(&observation),
    );
}

#[test]
fn activating_an_observation_moves_probe_policy_out_of_the_pending_slot() {
    let request = PendingObservation::new(
        ExternalDemand::new(
            IngressSeq::new(3),
            ExternalDemandKind::ExternalCursor,
            Millis::new(20),
            None,
            BufferPerfClass::Full,
        ),
        ProbeRequestSet::only(ProbeKind::CursorColor),
    );
    let observation = ObservationSnapshot::new(
        request.clone(),
        observation_basis(Some(cursor(7, 8)), 21),
        observation_motion(),
    );
    let observing = ready_state()
        .enter_observing_request(request)
        .expect("primed state should stage a collecting observation request")
        .with_active_observation(observation.clone())
        .expect("observation activation should keep the state on the observing path");

    assert_phase_views(
        &observing,
        Lifecycle::Observing,
        ProtocolPhaseKind::Observing,
    );
    assert_observation_views(
        &observing,
        None,
        Some(&observation),
        Some(&observation),
        None,
    );
}

#[test]
fn replacing_an_active_observation_with_pending_keeps_it_in_the_collecting_retained_slot() {
    let active = observation_snapshot(cursor(7, 8));
    let replacement = observation_request(17, ExternalDemandKind::ExternalCursor, 32);
    let mut observing = ready_state()
        .enter_observing_request(observation_request(
            3,
            ExternalDemandKind::ExternalCursor,
            20,
        ))
        .expect("primed state should stage a collecting observation request")
        .with_active_observation(active.clone())
        .expect("collecting state should activate an observation");

    assert!(
        observing.replace_active_observation_with_pending(replacement.clone()),
        "observing phase should demote the active observation into the collecting slot",
    );

    assert_phase_views(
        &observing,
        Lifecycle::Observing,
        ProtocolPhaseKind::Collecting,
    );
    assert_observation_views(
        &observing,
        Some(&replacement),
        None,
        Some(&active),
        Some(&active),
    );
}

#[test]
fn enter_planning_keeps_the_active_observation_inside_the_planning_phase() {
    let observation = observation_snapshot(cursor(11, 12));
    let ready = ready_state()
        .with_ready_observation(observation.clone())
        .expect("primed state should accept a ready observation");

    assert_phase_views(&ready, Lifecycle::Ready, ProtocolPhaseKind::Ready);
    assert_observation_views(&ready, None, Some(&observation), Some(&observation), None);

    let (ready, proposal_id) = ready.allocate_proposal_id();
    let planning = ready
        .enter_planning(proposal_id)
        .expect("ready state should enter planning");

    assert_phase_views(&planning, Lifecycle::Planning, ProtocolPhaseKind::Planning);
    assert_observation_views(
        &planning,
        None,
        Some(&observation),
        Some(&observation),
        None,
    );
}

#[test]
fn applying_and_ready_transitions_preserve_the_active_observation_inside_the_phase() {
    let observation = observation_snapshot(cursor(9, 10));
    let ready = ready_state()
        .with_ready_observation(observation.clone())
        .expect("primed state should accept a ready observation");
    let (applying, proposal_id) =
        applying_state_with_realization_plan(ready, noop_realization_plan(), false, None);

    assert_phase_views(&applying, Lifecycle::Applying, ProtocolPhaseKind::Applying);
    assert_observation_views(
        &applying,
        None,
        Some(&observation),
        Some(&observation),
        None,
    );

    let (restored_ready, _) = applying
        .clear_pending_for(proposal_id)
        .expect("clearing the pending proposal should return to ready");

    assert_phase_views(&restored_ready, Lifecycle::Ready, ProtocolPhaseKind::Ready);
    assert_observation_views(
        &restored_ready,
        None,
        Some(&observation),
        Some(&observation),
        None,
    );
}

#[test]
fn enter_recovering_moves_the_active_observation_into_retained_phase_payload() {
    let observation = observation_snapshot(cursor(13, 14));
    let recovering = ready_state()
        .with_ready_observation(observation.clone())
        .expect("primed state should accept a ready observation")
        .enter_recovering();

    assert_phase_views(
        &recovering,
        Lifecycle::Recovering,
        ProtocolPhaseKind::Recovering,
    );
    assert_observation_views(
        &recovering,
        None,
        None,
        Some(&observation),
        Some(&observation),
    );
}

#[test]
fn lifecycle_advancing_observation_boundaries_bump_generation() {
    let observation = observation_snapshot(cursor(13, 14));
    let mut collecting = ready_state()
        .enter_observing_request(observation_request(
            17,
            ExternalDemandKind::ExternalCursor,
            32,
        ))
        .expect("primed state should stage a collecting observation request");
    let collecting_generation = collecting.generation();
    assert!(collecting.enter_ready(observation.clone()));
    pretty_assert_eq!(collecting.generation(), collecting_generation.next());

    let mut observing = ready_state()
        .enter_observing_request(observation_request(
            19,
            ExternalDemandKind::ExternalCursor,
            34,
        ))
        .expect("primed state should stage a collecting observation request")
        .with_active_observation(observation.clone())
        .expect("collecting state should activate an observation");
    let observing_generation = observing.generation();
    assert!(observing.complete_active_observation());
    pretty_assert_eq!(observing.generation(), observing_generation.next());

    let ready = ready_state()
        .with_ready_observation(observation)
        .expect("primed state should accept a ready observation");
    let mut recovering = ready.enter_recovering();
    let recovering_generation = recovering.generation();
    assert!(recovering.restore_retained_observation_to_ready());
    pretty_assert_eq!(recovering.generation(), recovering_generation.next());
}
