use super::CursorVisibilityEffect;
use super::RenderAction;
use super::RenderSideEffects;
use super::TargetCellPresentation;
use crate::types::Point;
use crate::types::RenderFrame;
use nvimrs_nvim_utils::mode::is_cmdline_mode;

fn point_inside_target_bounds(
    point: Point,
    target_min_row: f64,
    target_max_row: f64,
    target_min_col: f64,
    target_max_col: f64,
) -> bool {
    point.row >= target_min_row
        && point.row <= target_max_row
        && point.col >= target_min_col
        && point.col <= target_max_col
}

fn frame_center(corners: &[Point; 4]) -> Point {
    let mut row = 0.0_f64;
    let mut col = 0.0_f64;
    for point in corners {
        row += point.row;
        col += point.col;
    }
    Point {
        row: row / 4.0,
        col: col / 4.0,
    }
}

fn frame_reaches_target_cell(frame: &RenderFrame) -> bool {
    let target_min_row = frame.target_corners[0].row;
    let target_max_row = frame.target_corners[2].row;
    let target_min_col = frame.target_corners[0].col;
    let target_max_col = frame.target_corners[2].col;
    let center = frame_center(&frame.corners);
    if point_inside_target_bounds(
        center,
        target_min_row,
        target_max_row,
        target_min_col,
        target_max_col,
    ) {
        return true;
    }

    frame.corners.iter().copied().any(|point| {
        point_inside_target_bounds(
            point,
            target_min_row,
            target_max_row,
            target_min_col,
            target_max_col,
        )
    })
}

pub(super) fn render_side_effects_for_action(
    mode: &str,
    render_action: &RenderAction,
    allow_real_cursor_updates: bool,
) -> RenderSideEffects {
    match render_action {
        RenderAction::Draw(frame) => {
            let cmdline_mode = is_cmdline_mode(mode);
            let should_show_cursor = cmdline_mode || frame_reaches_target_cell(frame);

            RenderSideEffects {
                // Cmdline rendering is event-driven and does not require forced redraws
                // for each animation frame.
                redraw_after_draw_if_cmdline: false,
                redraw_after_clear_if_cmdline: false,
                target_cell_presentation: if !cmdline_mode
                    && !should_show_cursor
                    && frame.hide_target_hack
                {
                    TargetCellPresentation::OverlayBlockCell
                } else {
                    TargetCellPresentation::None
                },
                cursor_visibility: if should_show_cursor {
                    CursorVisibilityEffect::Show
                } else if !cmdline_mode {
                    CursorVisibilityEffect::Hide
                } else {
                    CursorVisibilityEffect::Keep
                },
                allow_real_cursor_updates: !frame.hide_target_hack,
            }
        }
        RenderAction::ClearAll => RenderSideEffects {
            redraw_after_draw_if_cmdline: false,
            redraw_after_clear_if_cmdline: is_cmdline_mode(mode),
            target_cell_presentation: TargetCellPresentation::None,
            cursor_visibility: CursorVisibilityEffect::Show,
            allow_real_cursor_updates,
        },
        RenderAction::Noop => RenderSideEffects {
            allow_real_cursor_updates,
            ..RenderSideEffects::default()
        },
    }
}
