use super::super::event_loop::EventLoopState;
use pretty_assertions::assert_eq;

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
