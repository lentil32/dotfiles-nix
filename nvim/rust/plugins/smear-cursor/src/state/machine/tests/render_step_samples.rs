use super::*;
use pretty_assertions::assert_eq;

#[test]
fn render_step_sample_scratch_reuses_returned_buffer() {
    let mut state = RuntimeState::default();
    let mut scratch = state.take_render_step_samples_scratch();
    scratch.reserve(4);
    scratch.push(RenderStepSample::new([point(3.0, 4.0); 4], 16.0));
    let scratch_capacity = scratch.capacity();
    let scratch_ptr = scratch.as_ptr();

    state.reclaim_render_step_samples_scratch(scratch);

    let reused_scratch = state.take_render_step_samples_scratch();
    assert_eq!(reused_scratch.capacity(), scratch_capacity);
    assert_eq!(reused_scratch.as_ptr(), scratch_ptr);
    assert!(reused_scratch.is_empty());
}

#[test]
fn runtime_state_clone_resets_retained_render_step_sample_scratch() {
    let mut state = RuntimeState::default();
    let mut scratch = state.take_render_step_samples_scratch();
    scratch.reserve(4);
    scratch.push(RenderStepSample::new([point(3.0, 4.0); 4], 16.0));
    state.reclaim_render_step_samples_scratch(scratch);

    let cloned = state.clone();

    assert!(state.render_step_samples_scratch_capacity() > 0);
    assert_eq!(cloned.render_step_samples_scratch_capacity(), 0);
}
