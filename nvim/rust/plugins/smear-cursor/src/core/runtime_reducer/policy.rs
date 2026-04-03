use crate::config::RuntimeConfig;
use crate::core::runtime_reducer::MotionClass;
use crate::core::state::SemanticEvent;
use crate::state::CursorLocation;
use crate::state::RuntimeState;
use crate::types::EPSILON;
use crate::types::Point;
use crate::types::display_metric_row_scale;
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

fn jump_cue_min_display_distance(config: &RuntimeConfig) -> f64 {
    if config.jump_cue_min_display_distance.is_finite() {
        config.jump_cue_min_display_distance.max(0.0)
    } else {
        0.0
    }
}

fn classify_motion_class(
    state: &RuntimeState,
    window_changed: bool,
    buffer_changed: bool,
    target_row: f64,
    target_col: f64,
) -> MotionClass {
    let current_target = state.target_position();
    let display_distance = current_target.display_distance(
        Point {
            row: target_row,
            col: target_col,
        },
        state.config.block_aspect_ratio,
    );
    let surface_changed = window_changed || buffer_changed;
    let cross_window = window_changed;

    if state.config.jump_cues_enabled
        && display_distance >= jump_cue_min_display_distance(&state.config)
        && (!cross_window || state.config.cross_window_jump_bridges)
    {
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

    let current_corners = state.current_corners();
    let current_row = current_corners[0].row;
    let current_col = current_corners[0].col;
    let delta_row = (target_row - current_row).abs();
    let delta_col = (target_col - current_col).abs();
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

fn same_window_and_buffer(left: &CursorLocation, right: &CursorLocation) -> bool {
    left.window_handle == right.window_handle && left.buffer_handle == right.buffer_handle
}

pub(crate) fn select_event_source(
    mode: &str,
    state: &RuntimeState,
    semantic_event: SemanticEvent,
    requested_target: Option<Point>,
    cursor_location: &CursorLocation,
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
    let Some(target) = requested_target else {
        return super::EventSource::AnimationTick;
    };
    if !state.is_initialized() {
        return super::EventSource::External;
    }
    let target_changed = state.target_position().distance_squared(target) > EPSILON;
    if target_changed {
        let same_surface = state
            .tracked_location_ref()
            .is_some_and(|location| same_window_and_buffer(location, cursor_location));
        if (state.is_animating() || state.is_settling()) && same_surface {
            return super::EventSource::AnimationTick;
        }
        return super::EventSource::External;
    }
    match state.tracked_location_ref() {
        Some(location) if location == cursor_location => super::EventSource::AnimationTick,
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
    use crate::state::CursorShape;

    fn initialized_state(block_aspect_ratio: f64) -> RuntimeState {
        let mut state = RuntimeState::default();
        state.config.block_aspect_ratio = block_aspect_ratio;
        state.config.jump_cue_min_display_distance = 8.0;
        state.initialize_cursor(
            Point { row: 5.0, col: 5.0 },
            CursorShape::new(false, false),
            7,
            &CursorLocation::new(10, 20, 1, 1),
        );
        state
    }

    #[test]
    fn jump_thresholds_use_display_metric_distance() {
        let mut state = initialized_state(2.0);
        state.config.min_vertical_distance_smear = 1.0;
        state.config.min_horizontal_distance_smear = 0.25;

        assert!(
            !should_jump_to_target(&state, false, false, 5.6, 5.2),
            "0.6 rows becomes 1.2 display cells at aspect 2.0 and should exceed the threshold"
        );
    }

    #[test]
    fn jump_thresholds_match_for_equal_display_space_motion() {
        let mut aspect_one = initialized_state(1.0);
        aspect_one.config.min_vertical_distance_smear = 1.0;
        aspect_one.config.min_horizontal_distance_smear = 0.25;

        let mut aspect_two = initialized_state(2.0);
        aspect_two.config.min_vertical_distance_smear = 1.0;
        aspect_two.config.min_horizontal_distance_smear = 0.25;

        assert_eq!(
            should_jump_to_target(&aspect_one, false, false, 5.8, 5.2),
            should_jump_to_target(&aspect_two, false, false, 5.4, 5.2)
        );
    }

    #[test]
    fn segmentation_matches_for_equal_display_space_motion() {
        let mut aspect_one = initialized_state(1.0);
        aspect_one.config.min_vertical_distance_smear = 1.0;
        aspect_one.config.min_horizontal_distance_smear = 0.25;

        let mut aspect_two = initialized_state(2.0);
        aspect_two.config.min_vertical_distance_smear = 1.0;
        aspect_two.config.min_horizontal_distance_smear = 0.25;

        assert_eq!(
            classify_target_transition(&aspect_one, false, false, 5.8, 5.2),
            classify_target_transition(&aspect_two, false, false, 5.4, 5.2)
        );
    }

    #[test]
    fn large_cross_window_moves_classify_as_discontinuous_jump() {
        let state = initialized_state(2.0);

        let decision = classify_target_transition(&state, true, false, 5.0, 25.0);

        assert_eq!(decision.motion_class, MotionClass::DiscontinuousJump);
    }

    #[test]
    fn surface_change_without_large_display_distance_classifies_as_surface_retarget() {
        let state = initialized_state(2.0);

        let decision = classify_target_transition(&state, true, false, 5.0, 8.0);

        assert_eq!(decision.motion_class, MotionClass::SurfaceRetarget);
    }
}
