use super::*;
use crate::position::ViewportBounds;

#[test]
fn render_plan_leaves_target_cell_available_for_cursor_punch_through() {
    let frame = base_frame();
    let viewport = ViewportBounds::new(200, 200).expect("positive viewport bounds");

    let output = render_frame_to_plan(&frame, PlannerState::default(), viewport);
    let target = (
        frame.target.row.round() as i64,
        frame.target.col.round() as i64,
    );

    assert!(
        output
            .plan
            .cell_ops
            .iter()
            .all(|op| (op.row, op.col) != target),
        "target cell should remain available for cursor punch-through"
    );
}

#[test]
fn trail_stroke_change_preserves_old_tail_without_bridging() {
    let viewport = ViewportBounds::new(200, 200).expect("positive viewport bounds");

    let mut first = base_frame();
    first.target.col = 8.0;
    set_frame_corners(
        &mut first,
        [
            RenderPoint {
                row: 10.0,
                col: 8.0,
            },
            RenderPoint {
                row: 10.0,
                col: 9.0,
            },
            RenderPoint {
                row: 11.0,
                col: 9.0,
            },
            RenderPoint {
                row: 11.0,
                col: 8.0,
            },
        ],
    );

    let mut second = first.clone();
    second.target.col = 24.0;
    set_frame_corners(
        &mut second,
        [
            RenderPoint {
                row: 10.0,
                col: 24.0,
            },
            RenderPoint {
                row: 10.0,
                col: 25.0,
            },
            RenderPoint {
                row: 11.0,
                col: 25.0,
            },
            RenderPoint {
                row: 11.0,
                col: 24.0,
            },
        ],
    );

    let after_first = render_frame_to_plan(&first, PlannerState::default(), viewport).next_state;

    let mut bridged = second.clone();
    bridged.trail_stroke_id = first.trail_stroke_id;
    let bridged_output = render_frame_to_plan(&bridged, after_first.clone(), viewport);

    let mut reset = second;
    reset.trail_stroke_id = StrokeId::new(first.trail_stroke_id.value().wrapping_add(1));
    let reset_output = render_frame_to_plan(&reset, after_first, viewport);

    let bridged_cells = op_cells(&bridged_output);
    let reset_cells = op_cells(&reset_output);

    assert_eq!(bridged_output.next_state.step_index.value(), 2);
    assert_eq!(reset_output.next_state.step_index.value(), 2);
    assert!(
        bridged_cells.len() >= reset_cells.len(),
        "stroke reset should avoid carrying broad bridge coverage"
    );
    assert!(
        reset_cells
            .iter()
            .all(|(row, col)| !((10..=11).contains(row) && (12..=20).contains(col))),
        "stroke reset should not draw an impossible bridge through the interior gap"
    );
    assert!(
        reset_output.next_state.history.len() >= 2,
        "disconnect should keep prior deposited slices alive so the old tail can fade"
    );
}
