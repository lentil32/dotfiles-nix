use super::*;
use crate::test_support::proptest::stateful_config;

#[derive(Clone, Copy, Debug)]
enum SurfaceChange {
    WindowOnly,
    BufferOnly,
    WindowAndBuffer,
}

impl SurfaceChange {
    fn changes_window(self) -> bool {
        matches!(self, Self::WindowOnly | Self::WindowAndBuffer)
    }

    fn changes_buffer(self) -> bool {
        matches!(self, Self::BufferOnly | Self::WindowAndBuffer)
    }
}

#[derive(Clone, Copy, Debug)]
enum SurfaceMotion {
    SurfaceRetarget,
    DiscontinuousJump,
}

impl SurfaceMotion {
    fn target_col(self) -> f64 {
        match self {
            Self::SurfaceRetarget => 18.0,
            Self::DiscontinuousJump => 46.0,
        }
    }

    fn expected_motion_class(self) -> MotionClass {
        match self {
            Self::SurfaceRetarget => MotionClass::SurfaceRetarget,
            Self::DiscontinuousJump => MotionClass::DiscontinuousJump,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct SurfaceHopCase {
    change: SurfaceChange,
    motion: SurfaceMotion,
    smear_between_windows: bool,
    smear_between_buffers: bool,
}

fn surface_hop_case() -> impl Strategy<Value = SurfaceHopCase> {
    (
        prop_oneof![
            Just(SurfaceChange::WindowOnly),
            Just(SurfaceChange::BufferOnly),
            Just(SurfaceChange::WindowAndBuffer),
        ],
        prop_oneof![
            Just(SurfaceMotion::SurfaceRetarget),
            Just(SurfaceMotion::DiscontinuousJump),
        ],
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(change, motion, smear_between_windows, smear_between_buffers)| SurfaceHopCase {
                change,
                motion,
                smear_between_windows,
                smear_between_buffers,
            },
        )
}

proptest! {
    #![proptest_config(stateful_config())]

    #[test]
    fn prop_surface_hops_either_clear_or_start_a_new_trail_stroke(case in surface_hop_case()) {
        let mut state = RuntimeState::default();
        state.config.delay_event_to_smear = 0.0;
        state.config.smear_between_windows = case.smear_between_windows;
        state.config.smear_between_buffers = case.smear_between_buffers;

        let _ = reduce_cursor_event(&mut state, "n", event(5.0, 6.0), EventSource::External);
        let before_stroke_id = state.trail_stroke_id();

        let window_handle = if case.change.changes_window() { 99 } else { 10 };
        let buffer_handle = if case.change.changes_buffer() { 999 } else { 20 };
        let expected_location = TrackedCursor::fixture(window_handle, buffer_handle, 1, 1);
        let target_position = RenderPoint {
            row: 5.0,
            col: case.motion.target_col(),
        };
        let transition = reduce_cursor_event(
            &mut state,
            "n",
            event_with_location(
                target_position.row,
                target_position.col,
                120.0,
                99,
                window_handle,
                buffer_handle,
            ),
            EventSource::External,
        );
        let expected_clear = (case.change.changes_window() && !case.smear_between_windows)
            || (case.change.changes_buffer() && !case.smear_between_buffers);

        prop_assert_eq!(transition.motion_class, case.motion.expected_motion_class());
        prop_assert_eq!(state.tracked_cursor(), Some(expected_location));
        prop_assert_eq!(state.target_position(), target_position);
        prop_assert!(state.trail_stroke_id() > before_stroke_id);
        if expected_clear {
            prop_assert!(matches!(render_action(&transition), RenderAction::ClearAll));
            prop_assert!(!transition.should_schedule_next_animation());
            prop_assert_eq!(
                render_cleanup_action(&transition),
                RenderCleanupAction::Schedule
            );
            prop_assert!(!state.is_animating());
        } else {
            let frame = draw_frame(&transition)
                .expect("surface hop with smearing enabled should keep drawing");

            prop_assert!(matches!(render_action(&transition), RenderAction::Draw(_)));
            prop_assert!(transition.should_schedule_next_animation());
            prop_assert_eq!(
                render_cleanup_action(&transition),
                RenderCleanupAction::Invalidate
            );
            prop_assert!(state.is_animating());
            prop_assert_eq!(frame.trail_stroke_id, state.trail_stroke_id());
        }
    }
}
