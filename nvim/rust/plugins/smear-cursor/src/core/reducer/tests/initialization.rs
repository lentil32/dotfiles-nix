use super::*;

#[test]
fn initialize_from_idle_enters_primed_protocol_without_follow_up_reads() {
    let state = CoreState::default();

    let transition = reduce(
        &state,
        Event::Initialize(InitializeEvent {
            observed_at: Millis::new(11),
        }),
    );

    pretty_assert_eq!(transition.next.lifecycle(), Lifecycle::Primed);
    assert!(transition.effects.is_empty());
}
