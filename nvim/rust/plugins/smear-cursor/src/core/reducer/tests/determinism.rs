use super::*;

fn reduce_event_sequence(initial: CoreState, events: &[Event]) -> Vec<Transition> {
    let mut state = initial;
    events
        .iter()
        .cloned()
        .map(|event| {
            let transition = reduce(&state, event);
            state = transition.next.clone();
            transition
        })
        .collect()
}

#[test]
fn same_core_state_and_event_sequence_produces_the_same_transitions() {
    let initial = CoreState::default();
    let request = observation_request(
        /*seq*/ 1,
        ExternalDemandKind::ExternalCursor,
        /*observed_at*/ 21,
    );
    let events = vec![
        Event::Initialize(InitializeEvent {
            observed_at: Millis::new(/*value*/ 11),
        }),
        external_demand_event(ExternalDemandKind::ExternalCursor, /*observed_at*/ 21),
        Event::ObservationBaseCollected(ObservationBaseCollectedEvent {
            observation_id: request.observation_id(),
            basis: observation_basis(Some(cursor(/*row*/ 7, /*col*/ 8)), /*observed_at*/ 22),
            cursor_color_probe_generations: None,
            motion: observation_motion(),
        }),
    ];

    let first_run = reduce_event_sequence(initial.clone(), &events);
    let second_run = reduce_event_sequence(initial, &events);

    pretty_assert_eq!(second_run, first_run);
}
