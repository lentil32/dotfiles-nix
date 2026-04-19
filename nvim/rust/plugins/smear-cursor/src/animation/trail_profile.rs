fn cursor_height_for_trail_threshold(vertical_bar: bool, horizontal_bar: bool) -> f64 {
    match (vertical_bar, horizontal_bar) {
        (_, true) => 1.0 / 8.0,
        _ => 1.0,
    }
}

pub(crate) fn scaled_corners_for_trail(
    corners: &[RenderPoint; 4],
    trail_thickness: f64,
    trail_thickness_x: f64,
) -> [RenderPoint; 4] {
    if !trail_thickness.is_finite()
        || trail_thickness < 0.0
        || !trail_thickness_x.is_finite()
        || trail_thickness_x < 0.0
    {
        return *corners;
    }

    let center = center(corners);
    let mut scaled = [RenderPoint::ZERO; 4];
    for (index, corner) in corners.iter().copied().enumerate() {
        let row_offset = corner.row - center.row;
        let col_offset = corner.col - center.col;
        scaled[index] = RenderPoint {
            row: center.row + row_offset * trail_thickness,
            col: center.col + col_offset * trail_thickness_x,
        };
    }
    scaled
}
