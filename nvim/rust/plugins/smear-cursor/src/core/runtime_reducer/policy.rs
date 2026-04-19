use crate::config::RuntimeConfig;
use crate::core::runtime_reducer::MotionClass;
use crate::core::runtime_reducer::MotionTarget;
use crate::core::state::SemanticEvent;
use crate::position::RenderPoint;
use crate::position::display_metric_row_scale;
use crate::state::RuntimeState;
use crate::state::TrackedCursor;
use crate::types::EPSILON;
use nvimrs_nvim_utils::mode::is_cmdline_mode;
use nvimrs_nvim_utils::mode::is_insert_like_mode;
use nvimrs_nvim_utils::mode::is_replace_like_mode;
use nvimrs_nvim_utils::mode::is_terminal_like_mode;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct PathSegmentationDecision {
    pub(super) should_jump: bool,
    pub(super) motion_class: MotionClass,
    pub(super) starts_new_trail_stroke: bool,
}

const DISCONTINUOUS_JUMP_BRIDGE_MIN_DISPLAY_DISTANCE: f64 = 16.0;

fn classify_motion_class(
    state: &RuntimeState,
    window_changed: bool,
    buffer_changed: bool,
    target_row: f64,
    target_col: f64,
) -> MotionClass {
    let current_target = state.target_position();
    let display_distance = current_target.display_distance(
        RenderPoint {
            row: target_row,
            col: target_col,
        },
        state.config.block_aspect_ratio,
    );
    let surface_changed = window_changed || buffer_changed;
    if display_distance >= DISCONTINUOUS_JUMP_BRIDGE_MIN_DISPLAY_DISTANCE {
        MotionClass::DiscontinuousJump
    } else if surface_changed {
        MotionClass::SurfaceRetarget
    } else {
        MotionClass::Continuous
    }
}

pub(super) fn should_jump_to_target(
    state: &RuntimeState,
    window_changed: bool,
    buffer_changed: bool,
    target_row: f64,
    target_col: f64,
) -> bool {
    if window_changed && !state.config.smear_between_windows {
        return true;
    }
    if buffer_changed && !state.config.smear_between_buffers {
        return true;
    }

    let current_anchor = state.current_visual_cursor_anchor();
    let delta_row = (target_row - current_anchor.row).abs();
    let delta_col = (target_col - current_anchor.col).abs();
    let row_scale = display_metric_row_scale(state.config.block_aspect_ratio);
    let display_delta_row = delta_row * row_scale;
    (!state.config.smear_between_neighbor_lines && display_delta_row <= 1.5 * row_scale)
        || (display_delta_row < state.config.min_vertical_distance_smear
            && delta_col < state.config.min_horizontal_distance_smear)
        || (!state.config.smear_horizontally && display_delta_row <= 0.5 * row_scale)
        || (!state.config.smear_vertically && delta_col <= 0.5)
        || (!state.config.smear_diagonally
            && display_delta_row > 0.5 * row_scale
            && delta_col > 0.5)
}

pub(super) fn classify_target_transition(
    state: &RuntimeState,
    window_changed: bool,
    buffer_changed: bool,
    target_row: f64,
    target_col: f64,
) -> PathSegmentationDecision {
    let should_jump = should_jump_to_target(
        state,
        window_changed,
        buffer_changed,
        target_row,
        target_col,
    );
    let motion_class = classify_motion_class(
        state,
        window_changed,
        buffer_changed,
        target_row,
        target_col,
    );
    PathSegmentationDecision {
        should_jump,
        motion_class,
        starts_new_trail_stroke: should_jump || window_changed || buffer_changed,
    }
}

fn same_window_and_buffer(left: &TrackedCursor, right: &TrackedCursor) -> bool {
    left.same_window_and_buffer(right)
}

pub(crate) fn select_event_source(
    mode: &str,
    state: &RuntimeState,
    semantic_event: SemanticEvent,
    motion_target: MotionTarget,
    tracked_cursor: &TrackedCursor,
) -> super::EventSource {
    if is_cmdline_mode(mode) {
        return super::EventSource::External;
    }
    if matches!(
        semantic_event,
        SemanticEvent::ModeChanged
            | SemanticEvent::TextMutatedAtCursorContext
            | SemanticEvent::ViewportOrWindowMoved
    ) {
        return super::EventSource::External;
    }
    let target = match motion_target {
        MotionTarget::Available(target_cell) => RenderPoint::from(target_cell),
        MotionTarget::Unavailable => match state.tracked_cursor_ref() {
            Some(current) if current == tracked_cursor => {
                return super::EventSource::AnimationTick;
            }
            Some(_) | None => return super::EventSource::External,
        },
    };
    if !state.is_initialized() {
        return super::EventSource::External;
    }
    let target_changed = state.target_position().distance_squared(target) > EPSILON;
    if target_changed {
        let same_surface = state
            .tracked_cursor_ref()
            .is_some_and(|current| same_window_and_buffer(current, tracked_cursor));
        if (state.is_animating() || state.is_settling()) && same_surface {
            return super::EventSource::AnimationTick;
        }
        return super::EventSource::External;
    }
    match state.tracked_cursor_ref() {
        Some(current) if current == tracked_cursor => super::EventSource::AnimationTick,
        Some(_) | None => super::EventSource::External,
    }
}

pub(super) fn external_mode_ignores_cursor(config: &RuntimeConfig, mode: &str) -> bool {
    is_cmdline_mode(mode) && !config.smear_to_cmd
}

pub(super) fn external_mode_requires_jump(config: &RuntimeConfig, mode: &str) -> bool {
    (is_insert_like_mode(mode) && !config.smear_insert_mode)
        || (is_replace_like_mode(mode) && !config.smear_replace_mode)
        || (is_terminal_like_mode(mode) && !config.smear_terminal_mode)
}

pub(super) fn external_mode_requires_immediate_movement(
    config: &RuntimeConfig,
    mode: &str,
    transitioned_to_or_from_cmdline: bool,
) -> bool {
    (is_insert_like_mode(mode) && !config.animate_in_insert_mode)
        || (!config.animate_command_line && transitioned_to_or_from_cmdline)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::runtime_reducer::EventSource;
    use crate::state::CursorShape;
    use crate::state::TrackedCursor;
    use crate::test_support::proptest::positive_aspect_ratio;
    use crate::test_support::proptest::pure_config;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn tracked_cursor(window_handle: i64, buffer_handle: i64, line: i64) -> TrackedCursor {
        TrackedCursor::fixture(window_handle, buffer_handle, 1, line)
    }

    fn initialized_state(block_aspect_ratio: f64) -> RuntimeState {
        let mut state = RuntimeState::default();
        state.config.block_aspect_ratio = block_aspect_ratio;
        state.initialize_cursor(
            RenderPoint { row: 5.0, col: 5.0 },
            CursorShape::block(),
            7,
            &tracked_cursor(10, 20, 1),
        );
        state
    }

    #[test]
    fn unavailable_target_with_changed_tracking_uses_external_source() {
        let state = initialized_state(1.0);

        assert_eq!(
            select_event_source(
                "n",
                &state,
                SemanticEvent::CursorMovedWithoutTextMutation,
                MotionTarget::Unavailable,
                &tracked_cursor(10, 20, 2),
            ),
            EventSource::External
        );
        assert_eq!(
            select_event_source(
                "n",
                &state,
                SemanticEvent::FrameCommitted,
                MotionTarget::Unavailable,
                &tracked_cursor(10, 20, 1),
            ),
            EventSource::AnimationTick
        );
    }

    fn configure_jump_thresholds(
        state: &mut RuntimeState,
        min_vertical_distance_smear: f64,
        min_horizontal_distance_smear: f64,
    ) {
        state.config.min_vertical_distance_smear = min_vertical_distance_smear;
        state.config.min_horizontal_distance_smear = min_horizontal_distance_smear;
    }

    fn target_for_display_motion(
        block_aspect_ratio: f64,
        display_delta_row: f64,
        delta_col: f64,
    ) -> RenderPoint {
        RenderPoint {
            row: 5.0 + (display_delta_row / block_aspect_ratio),
            col: 5.0 + delta_col,
        }
    }

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_jump_and_segmentation_match_for_equal_display_space_motion(
            aspect_one in positive_aspect_ratio(),
            aspect_two in positive_aspect_ratio(),
            display_delta_row in 0.0_f64..6.0_f64,
            delta_col in 0.0_f64..6.0_f64,
            min_vertical_distance_smear in 0.0_f64..6.0_f64,
            min_horizontal_distance_smear in 0.0_f64..6.0_f64,
        ) {
            let mut state_one = initialized_state(aspect_one);
            configure_jump_thresholds(
                &mut state_one,
                min_vertical_distance_smear,
                min_horizontal_distance_smear,
            );
            let mut state_two = initialized_state(aspect_two);
            configure_jump_thresholds(
                &mut state_two,
                min_vertical_distance_smear,
                min_horizontal_distance_smear,
            );
            let target_one = target_for_display_motion(aspect_one, display_delta_row, delta_col);
            let target_two = target_for_display_motion(aspect_two, display_delta_row, delta_col);

            prop_assert_eq!(
                should_jump_to_target(
                    &state_one,
                    false,
                    false,
                    target_one.row,
                    target_one.col
                ),
                should_jump_to_target(
                    &state_two,
                    false,
                    false,
                    target_two.row,
                    target_two.col
                )
            );
            prop_assert_eq!(
                classify_target_transition(
                    &state_one,
                    false,
                    false,
                    target_one.row,
                    target_one.col
                ),
                classify_target_transition(
                    &state_two,
                    false,
                    false,
                    target_two.row,
                    target_two.col
                )
            );
        }

        #[test]
        fn prop_cross_surface_classification_uses_display_distance_thresholds(
            aspect_one in positive_aspect_ratio(),
            aspect_two in positive_aspect_ratio(),
            display_delta_row in 0.0_f64..20.0_f64,
            delta_col in 0.0_f64..20.0_f64,
            window_changed in any::<bool>(),
            buffer_changed in any::<bool>(),
        ) {
            prop_assume!(window_changed || buffer_changed);

            let state_one = initialized_state(aspect_one);
            let state_two = initialized_state(aspect_two);
            let target_one = target_for_display_motion(aspect_one, display_delta_row, delta_col);
            let target_two = target_for_display_motion(aspect_two, display_delta_row, delta_col);
            let expected_motion_class = if display_delta_row.hypot(delta_col)
                >= DISCONTINUOUS_JUMP_BRIDGE_MIN_DISPLAY_DISTANCE
            {
                MotionClass::DiscontinuousJump
            } else {
                MotionClass::SurfaceRetarget
            };
            let decision_one = classify_target_transition(
                &state_one,
                window_changed,
                buffer_changed,
                target_one.row,
                target_one.col,
            );
            let decision_two = classify_target_transition(
                &state_two,
                window_changed,
                buffer_changed,
                target_two.row,
                target_two.col,
            );

            prop_assert_eq!(decision_one, decision_two);
            prop_assert_eq!(decision_one.motion_class, expected_motion_class);
            prop_assert!(decision_one.starts_new_trail_stroke);
        }
    }
}
