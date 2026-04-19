use super::CursorVisibilityEffect;
use super::RenderAction;
use super::RenderSideEffects;
use super::TargetCellPresentation;
use crate::types::CursorCellShape;
use crate::types::Point;
use crate::types::RenderFrame;
use nvimrs_nvim_utils::mode::is_cmdline_mode;

fn bounds_for_corners(corners: &[Point; 4]) -> (f64, f64, f64, f64) {
    let mut min_row = f64::INFINITY;
    let mut max_row = f64::NEG_INFINITY;
    let mut min_col = f64::INFINITY;
    let mut max_col = f64::NEG_INFINITY;

    for corner in corners {
        min_row = min_row.min(corner.row);
        max_row = max_row.max(corner.row);
        min_col = min_col.min(corner.col);
        max_col = max_col.max(corner.col);
    }

    (min_row, max_row, min_col, max_col)
}

fn intervals_overlap_with_positive_area(
    first_min: f64,
    first_max: f64,
    second_min: f64,
    second_max: f64,
) -> bool {
    first_min < second_max && second_min < first_max
}

fn frame_reaches_target_cell(frame: &RenderFrame) -> bool {
    let (target_min_row, target_max_row, target_min_col, target_max_col) =
        bounds_for_corners(&frame.target_corners);
    let (frame_min_row, frame_max_row, frame_min_col, frame_max_col) =
        bounds_for_corners(&frame.corners);

    intervals_overlap_with_positive_area(
        frame_min_row,
        frame_max_row,
        target_min_row,
        target_max_row,
    ) && intervals_overlap_with_positive_area(
        frame_min_col,
        frame_max_col,
        target_min_col,
        target_max_col,
    )
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
                    TargetCellPresentation::OverlayCursorCell(CursorCellShape::from_corners(
                        &frame.target_corners,
                    ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RuntimeConfig;
    use crate::core::types::StrokeId;
    use crate::test_support::proptest::CursorShapeCase;
    use crate::test_support::proptest::cursor_rectangle;
    use crate::test_support::proptest::pure_config;
    use crate::types::ModeClass;
    use crate::types::RenderStepSample;
    use crate::types::StaticRenderConfig;
    use proptest::prelude::*;
    use std::sync::Arc;

    fn rectangle(min_row: f64, max_row: f64, min_col: f64, max_col: f64) -> [Point; 4] {
        [
            Point {
                row: min_row,
                col: min_col,
            },
            Point {
                row: min_row,
                col: max_col,
            },
            Point {
                row: max_row,
                col: max_col,
            },
            Point {
                row: max_row,
                col: min_col,
            },
        ]
    }

    fn test_frame(
        corners: [Point; 4],
        target_corners: [Point; 4],
        hide_target_hack: bool,
    ) -> RenderFrame {
        let mut static_config = StaticRenderConfig::from(&RuntimeConfig::default());
        static_config.hide_target_hack = hide_target_hack;

        RenderFrame {
            mode: ModeClass::NormalLike,
            corners,
            step_samples: Vec::<RenderStepSample>::new().into(),
            planner_idle_steps: 0,
            target: Point {
                row: target_corners[0].row,
                col: target_corners[0].col,
            },
            target_corners,
            vertical_bar: false,
            trail_stroke_id: StrokeId::INITIAL,
            retarget_epoch: 0,
            particle_count: 0,
            aggregated_particle_cells: Arc::default(),
            particle_screen_cells: Arc::default(),
            color_at_cursor: None,
            static_config: Arc::new(static_config),
        }
    }

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_border_touch_keeps_target_overlay_visible_until_interior_overlap(
            target_row in 1_i64..128_i64,
            target_col in 1_i64..128_i64,
            target_height in 1_i64..8_i64,
            target_width in 1_i64..8_i64,
            frame_width in 1_i64..8_i64,
        ) {
            let target_min_row = target_row as f64;
            let target_max_row = (target_row + target_height) as f64;
            let target_min_col = target_col as f64;
            let target_max_col = (target_col + target_width) as f64;
            let target_corners = rectangle(
                target_min_row,
                target_max_row,
                target_min_col,
                target_max_col,
            );
            let border_touch_frame = test_frame(
                rectangle(
                    target_min_row,
                    target_max_row,
                    target_min_col - frame_width as f64,
                    target_min_col,
                ),
                target_corners,
                true,
            );
            let interior_overlap_frame = test_frame(
                rectangle(
                    target_min_row,
                    target_max_row,
                    target_min_col - frame_width as f64 + 0.5,
                    target_min_col + 0.5,
                ),
                target_corners,
                true,
            );

            let border_touch = render_side_effects_for_action(
                "n",
                &RenderAction::Draw(Box::new(border_touch_frame)),
                /*allow_real_cursor_updates*/ false,
            );
            let interior_overlap = render_side_effects_for_action(
                "n",
                &RenderAction::Draw(Box::new(interior_overlap_frame)),
                /*allow_real_cursor_updates*/ false,
            );

            prop_assert_eq!(border_touch.cursor_visibility, CursorVisibilityEffect::Hide);
            prop_assert_eq!(
                border_touch.target_cell_presentation,
                TargetCellPresentation::OverlayCursorCell(CursorCellShape::Block)
            );
            prop_assert_eq!(interior_overlap.cursor_visibility, CursorVisibilityEffect::Show);
            prop_assert_eq!(
                interior_overlap.target_cell_presentation,
                TargetCellPresentation::None
            );
        }

        #[test]
        fn prop_positive_area_overlap_counts_when_frame_fully_contains_target(
            target_row in 1_i64..128_i64,
            target_col in 1_i64..128_i64,
            target_height in 1_i64..8_i64,
            target_width in 1_i64..8_i64,
            row_padding in 1_i64..8_i64,
            col_padding in 1_i64..8_i64,
        ) {
            let target_corners = rectangle(
                target_row as f64,
                (target_row + target_height) as f64,
                target_col as f64,
                (target_col + target_width) as f64,
            );
            let wide_frame = test_frame(
                rectangle(
                    (target_row - row_padding) as f64,
                    (target_row + target_height + row_padding) as f64,
                    (target_col - col_padding) as f64,
                    (target_col + target_width + col_padding) as f64,
                ),
                target_corners,
                false,
            );

            prop_assert!(frame_reaches_target_cell(&wide_frame));
        }

        #[test]
        fn prop_target_hack_preserves_overlay_shape_before_overlap(
            fixture in cursor_rectangle(),
        ) {
            let frame = test_frame(
                rectangle(
                    fixture.position.row,
                    fixture.position.row + 1.0,
                    fixture.position.col - 1.0,
                    fixture.position.col,
                ),
                fixture.corners,
                true,
            );

            let side_effects = render_side_effects_for_action(
                "n",
                &RenderAction::Draw(Box::new(frame)),
                /*allow_real_cursor_updates*/ false,
            );

            let expected_shape = match fixture.shape {
                CursorShapeCase::Block => CursorCellShape::Block,
                CursorShapeCase::VerticalBar => CursorCellShape::VerticalBar,
                CursorShapeCase::HorizontalBar => CursorCellShape::HorizontalBar,
            };

            prop_assert_eq!(side_effects.cursor_visibility, CursorVisibilityEffect::Hide);
            prop_assert_eq!(
                side_effects.target_cell_presentation,
                TargetCellPresentation::OverlayCursorCell(expected_shape)
            );
        }
    }
}
