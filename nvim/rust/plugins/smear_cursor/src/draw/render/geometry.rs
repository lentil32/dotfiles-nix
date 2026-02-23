use super::super::{BLOCK_ASPECT_RATIO, RenderFrame};
use crate::types::Point;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum EdgeType {
    Top,
    Bottom,
    Left,
    Right,
    LeftDiagonal,
    RightDiagonal,
    None,
}

#[derive(Clone, Debug, Default)]
pub(super) struct EdgeIntersections {
    pub(super) centerlines: HashMap<i64, f64>,
    pub(super) edges: HashMap<i64, [f64; 2]>,
    pub(super) fractions: HashMap<i64, [f64; 2]>,
}

impl EdgeIntersections {
    fn with_capacities(centerlines: usize, edges: usize, fractions: usize) -> Self {
        Self {
            centerlines: HashMap::with_capacity(centerlines),
            edges: HashMap::with_capacity(edges),
            fractions: HashMap::with_capacity(fractions),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct QuadGeometry {
    pub(super) top: i64,
    pub(super) bottom: i64,
    pub(super) left: i64,
    pub(super) right: i64,
    pub(super) slopes: [f64; 4],
    pub(super) angles: [f64; 4],
    pub(super) edge_types: [EdgeType; 4],
    pub(super) intersections: [EdgeIntersections; 4],
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct CellIntersections {
    pub(super) top: Option<f64>,
    pub(super) bottom: Option<f64>,
    pub(super) left: Option<f64>,
    pub(super) right: Option<f64>,
    pub(super) diagonal: Option<f64>,
}

const DIAGONAL_SLOPES: [f64; 10] = [
    -2.0,
    -4.0 / 3.0,
    -1.0,
    -2.0 / 3.0,
    -1.0 / 3.0,
    1.0 / 3.0,
    2.0 / 3.0,
    1.0,
    4.0 / 3.0,
    2.0,
];

const LEFT_DIAGONAL_NEG_2: &[(f64, &str)] = &[(-1.0 / 4.0, "🭛"), (1.0 / 4.0, "🭟")];
const LEFT_DIAGONAL_NEG_4_3: &[(f64, &str)] = &[(-3.0 / 8.0, "🭙"), (3.0 / 8.0, "🭟")];
const LEFT_DIAGONAL_NEG_1: &[(f64, &str)] = &[(0.0, "◤")];
const LEFT_DIAGONAL_NEG_2_3: &[(f64, &str)] = &[
    (-3.0 / 4.0, "🭗"),
    (-1.0 / 4.0, "🭚"),
    (1.0 / 4.0, "🭠"),
    (3.0 / 4.0, "🭝"),
];
const LEFT_DIAGONAL_NEG_1_3: &[(f64, &str)] = &[(-1.0, "🭘"), (0.0, "🭜"), (1.0, "🭞")];
const LEFT_DIAGONAL_POS_1_3: &[(f64, &str)] = &[(-1.0, "🬽"), (0.0, "🭑"), (1.0, "🭍")];
const LEFT_DIAGONAL_POS_2_3: &[(f64, &str)] = &[
    (-3.0 / 4.0, "🬼"),
    (-1.0 / 4.0, "🬿"),
    (1.0 / 4.0, "🭏"),
    (3.0 / 4.0, "🭌"),
];
const LEFT_DIAGONAL_POS_1: &[(f64, &str)] = &[(0.0, "◣")];
const LEFT_DIAGONAL_POS_4_3: &[(f64, &str)] = &[(-3.0 / 8.0, "🬾"), (3.0 / 8.0, "🭎")];
const LEFT_DIAGONAL_POS_2: &[(f64, &str)] = &[(-1.0 / 4.0, "🭀"), (1.0 / 4.0, "🭐")];

const RIGHT_DIAGONAL_NEG_2: &[(f64, &str)] = &[(-1.0 / 4.0, "🭅"), (1.0 / 4.0, "🭋")];
const RIGHT_DIAGONAL_NEG_4_3: &[(f64, &str)] = &[(-3.0 / 8.0, "🭃"), (3.0 / 8.0, "🭉")];
const RIGHT_DIAGONAL_NEG_1: &[(f64, &str)] = &[(0.0, "◢")];
const RIGHT_DIAGONAL_NEG_2_3: &[(f64, &str)] = &[
    (-3.0 / 4.0, "🭁"),
    (-1.0 / 4.0, "🭄"),
    (1.0 / 4.0, "🭊"),
    (3.0 / 4.0, "🭇"),
];
const RIGHT_DIAGONAL_NEG_1_3: &[(f64, &str)] = &[(-1.0, "🭂"), (0.0, "🭆"), (1.0, "🭈")];
const RIGHT_DIAGONAL_POS_1_3: &[(f64, &str)] = &[(-1.0, "🭓"), (0.0, "🭧"), (1.0, "🭣")];
const RIGHT_DIAGONAL_POS_2_3: &[(f64, &str)] = &[
    (-3.0 / 4.0, "🭒"),
    (-1.0 / 4.0, "🭕"),
    (1.0 / 4.0, "🭥"),
    (3.0 / 4.0, "🭢"),
];
const RIGHT_DIAGONAL_POS_1: &[(f64, &str)] = &[(0.0, "◥")];
const RIGHT_DIAGONAL_POS_4_3: &[(f64, &str)] = &[(-3.0 / 8.0, "🭔"), (3.0 / 8.0, "🭤")];
const RIGHT_DIAGONAL_POS_2: &[(f64, &str)] = &[(-1.0 / 4.0, "🭖"), (1.0 / 4.0, "🭦")];

pub(super) fn frac01(value: f64) -> f64 {
    value.rem_euclid(1.0)
}

pub(super) fn round_lua(value: f64) -> i64 {
    (value + 0.5).floor() as i64
}

fn inclusive_span_len(start: i64, end: i64) -> usize {
    if end < start {
        return 0;
    }
    let width = end.saturating_sub(start).saturating_add(1);
    usize::try_from(width).unwrap_or(0)
}

pub(super) fn level_from_shade(shade: f64, color_levels: u32) -> u32 {
    if !shade.is_finite() || color_levels == 0 {
        return 0;
    }

    let rounded = round_lua(shade * f64::from(color_levels));
    if rounded <= 0 {
        0
    } else {
        let clamped = rounded.min(i64::from(color_levels));
        u32::try_from(clamped).unwrap_or(0)
    }
}

pub(super) fn ensure_clockwise(corners: &[Point; 4]) -> [Point; 4] {
    let cross = (corners[1].row - corners[0].row) * (corners[3].col - corners[0].col)
        - (corners[1].col - corners[0].col) * (corners[3].row - corners[0].row);

    if cross > 0.0 {
        [corners[2], corners[1], corners[0], corners[3]]
    } else {
        *corners
    }
}

fn precompute_intersections_horizontal(
    corners: &[Point; 4],
    geometry: &mut QuadGeometry,
    edge_index: usize,
) {
    let slope = geometry.slopes[edge_index];
    let corner = corners[edge_index];
    let intersections = &mut geometry.intersections[edge_index];

    for col in geometry.left..=geometry.right {
        let centerline = corner.row + ((col as f64 + 0.5) - corner.col) * slope;
        intersections.centerlines.insert(col, centerline);
        intersections
            .fractions
            .insert(col, [centerline - 0.25 * slope, centerline + 0.25 * slope]);
    }
}

fn precompute_intersections_vertical(
    corners: &[Point; 4],
    geometry: &mut QuadGeometry,
    edge_index: usize,
) {
    let slope = geometry.slopes[edge_index];
    let corner = corners[edge_index];
    let intersections = &mut geometry.intersections[edge_index];

    for row in geometry.top..=geometry.bottom {
        let centerline = corner.col + ((row as f64 + 0.5) - corner.row) / slope;
        intersections.centerlines.insert(row, centerline);
        intersections
            .fractions
            .insert(row, [centerline - 0.25 / slope, centerline + 0.25 / slope]);
    }
}

fn precompute_intersections_diagonal(
    corners: &[Point; 4],
    geometry: &mut QuadGeometry,
    edge_index: usize,
    frame: &RenderFrame,
) {
    let slope = geometry.slopes[edge_index];
    let edge_type = geometry.edge_types[edge_index];
    let corner = corners[edge_index];
    let intersections = &mut geometry.intersections[edge_index];

    for row in geometry.top..=geometry.bottom {
        let centerline = corner.col + ((row as f64 + 0.5) - corner.row) / slope;
        intersections.centerlines.insert(row, centerline);

        let (shift_1, shift_2) = if edge_type == EdgeType::LeftDiagonal {
            (-0.5, 0.5)
        } else {
            (0.5, -0.5)
        };

        intersections.edges.insert(
            row,
            [
                centerline + shift_1 / slope.abs(),
                centerline + shift_2 / slope.abs(),
            ],
        );
        intersections
            .fractions
            .insert(row, [centerline - 0.25 / slope, centerline + 0.25 / slope]);
    }

    let mut min_angle_difference = f64::INFINITY;
    let mut closest_slope = None;
    for block_slope in DIAGONAL_SLOPES {
        let angle_difference =
            ((BLOCK_ASPECT_RATIO * block_slope).atan() - geometry.angles[edge_index]).abs();
        if angle_difference < min_angle_difference {
            min_angle_difference = angle_difference;
            closest_slope = Some(block_slope);
        }
    }

    if let Some(slope) = closest_slope
        && min_angle_difference <= frame.max_angle_difference_diagonal
    {
        geometry.slopes[edge_index] = slope;
    }
}

pub(super) fn precompute_quad_geometry(corners: &[Point; 4], frame: &RenderFrame) -> QuadGeometry {
    let top = corners
        .iter()
        .fold(f64::INFINITY, |acc, corner| acc.min(corner.row))
        .floor() as i64;
    let bottom = corners
        .iter()
        .fold(f64::NEG_INFINITY, |acc, corner| acc.max(corner.row))
        .ceil() as i64
        - 1;
    let left = corners
        .iter()
        .fold(f64::INFINITY, |acc, corner| acc.min(corner.col))
        .floor() as i64;
    let right = corners
        .iter()
        .fold(f64::NEG_INFINITY, |acc, corner| acc.max(corner.col))
        .ceil() as i64
        - 1;

    let mut slopes = [0.0; 4];
    let mut angles = [0.0; 4];
    let mut edge_types = [EdgeType::None; 4];

    for edge_index in 0..4 {
        let next_index = (edge_index + 1) % 4;
        let edge_row = corners[next_index].row - corners[edge_index].row;
        let edge_col = corners[next_index].col - corners[edge_index].col;
        let slope = edge_row / edge_col;
        slopes[edge_index] = slope;
        angles[edge_index] = (BLOCK_ASPECT_RATIO * slope).atan();

        let abs_slope = slope.abs();
        edge_types[edge_index] = if abs_slope.is_nan() {
            EdgeType::None
        } else if abs_slope <= frame.max_slope_horizontal {
            if edge_col > 0.0 {
                EdgeType::Top
            } else {
                EdgeType::Bottom
            }
        } else if abs_slope >= frame.min_slope_vertical {
            if edge_row > 0.0 {
                EdgeType::Right
            } else {
                EdgeType::Left
            }
        } else if edge_row > 0.0 {
            EdgeType::RightDiagonal
        } else {
            EdgeType::LeftDiagonal
        };
    }

    let col_span = inclusive_span_len(left, right);
    let row_span = inclusive_span_len(top, bottom);
    let intersections = std::array::from_fn(|edge_index| match edge_types[edge_index] {
        EdgeType::Top | EdgeType::Bottom => {
            EdgeIntersections::with_capacities(col_span, 0, col_span)
        }
        EdgeType::Left | EdgeType::Right => {
            EdgeIntersections::with_capacities(row_span, 0, row_span)
        }
        EdgeType::LeftDiagonal | EdgeType::RightDiagonal => {
            EdgeIntersections::with_capacities(row_span, row_span, row_span)
        }
        EdgeType::None => EdgeIntersections::default(),
    });

    let mut geometry = QuadGeometry {
        top,
        bottom,
        left,
        right,
        slopes,
        angles,
        edge_types,
        intersections,
    };

    for edge_index in 0..4 {
        match geometry.edge_types[edge_index] {
            EdgeType::Top | EdgeType::Bottom => {
                precompute_intersections_horizontal(corners, &mut geometry, edge_index)
            }
            EdgeType::Left | EdgeType::Right => {
                precompute_intersections_vertical(corners, &mut geometry, edge_index)
            }
            EdgeType::LeftDiagonal | EdgeType::RightDiagonal => {
                precompute_intersections_diagonal(corners, &mut geometry, edge_index, frame)
            }
            EdgeType::None => {}
        }
    }

    geometry
}

pub(super) fn get_edge_cell_intersection(
    edge_index: usize,
    row: i64,
    col: i64,
    geometry: &QuadGeometry,
    low: bool,
) -> f64 {
    let intersections = &geometry.intersections[edge_index];
    match geometry.edge_types[edge_index] {
        EdgeType::Top => intersections
            .centerlines
            .get(&col)
            .map_or(0.0, |centerline| centerline - row as f64),
        EdgeType::Bottom => intersections
            .centerlines
            .get(&col)
            .map_or(0.0, |centerline| row as f64 + 1.0 - centerline),
        EdgeType::Left => intersections
            .centerlines
            .get(&row)
            .map_or(0.0, |centerline| centerline - col as f64),
        EdgeType::Right => intersections
            .centerlines
            .get(&row)
            .map_or(0.0, |centerline| col as f64 + 1.0 - centerline),
        EdgeType::LeftDiagonal => intersections
            .edges
            .get(&row)
            .map_or(0.0, |edges| edges[if low { 0 } else { 1 }] - col as f64),
        EdgeType::RightDiagonal => intersections.edges.get(&row).map_or(0.0, |edges| {
            col as f64 + 1.0 - edges[if low { 0 } else { 1 }]
        }),
        EdgeType::None => 0.0,
    }
}

fn update_matrix_with_top_edge(
    edge_index: usize,
    fraction_index: usize,
    row: i64,
    col: i64,
    geometry: &QuadGeometry,
    matrix: &mut [[f64; 2]; 2],
) {
    let Some(fractions) = geometry.intersections[edge_index].fractions.get(&col) else {
        return;
    };

    let row_float = 2.0 * (fractions[fraction_index] - row as f64);
    let matrix_index = row_float.floor() as i64 + 1;

    let upper = (matrix_index - 1).min(2);
    if upper >= 1 {
        for index in 1..=upper {
            matrix[(index - 1) as usize][fraction_index] = 0.0;
        }
    }

    if matrix_index == 1 || matrix_index == 2 {
        let shade = 1.0 - row_float.rem_euclid(1.0);
        matrix[(matrix_index - 1) as usize][fraction_index] *= shade;
    }
}

fn update_matrix_with_bottom_edge(
    edge_index: usize,
    fraction_index: usize,
    row: i64,
    col: i64,
    geometry: &QuadGeometry,
    matrix: &mut [[f64; 2]; 2],
) {
    let Some(fractions) = geometry.intersections[edge_index].fractions.get(&col) else {
        return;
    };

    let row_float = 2.0 * (fractions[fraction_index] - row as f64);
    let matrix_index = row_float.floor() as i64 + 1;

    let start = (matrix_index + 1).max(1);
    if start <= 2 {
        for index in start..=2 {
            matrix[(index - 1) as usize][fraction_index] = 0.0;
        }
    }

    if matrix_index == 1 || matrix_index == 2 {
        let shade = row_float.rem_euclid(1.0);
        matrix[(matrix_index - 1) as usize][fraction_index] *= shade;
    }
}

fn update_matrix_with_left_edge(
    edge_index: usize,
    fraction_index: usize,
    row: i64,
    col: i64,
    geometry: &QuadGeometry,
    matrix: &mut [[f64; 2]; 2],
) {
    let Some(fractions) = geometry.intersections[edge_index].fractions.get(&row) else {
        return;
    };

    let col_float = 2.0 * (fractions[fraction_index] - col as f64);
    let matrix_index = col_float.floor() as i64 + 1;

    let upper = (matrix_index - 1).min(2);
    if upper >= 1 {
        for index in 1..=upper {
            matrix[fraction_index][(index - 1) as usize] = 0.0;
        }
    }

    if matrix_index == 1 || matrix_index == 2 {
        let shade = 1.0 - col_float.rem_euclid(1.0);
        matrix[fraction_index][(matrix_index - 1) as usize] *= shade;
    }
}

fn update_matrix_with_right_edge(
    edge_index: usize,
    fraction_index: usize,
    row: i64,
    col: i64,
    geometry: &QuadGeometry,
    matrix: &mut [[f64; 2]; 2],
) {
    let Some(fractions) = geometry.intersections[edge_index].fractions.get(&row) else {
        return;
    };

    let col_float = 2.0 * (fractions[fraction_index] - col as f64);
    let matrix_index = col_float.floor() as i64 + 1;

    let start = (matrix_index + 1).max(1);
    if start <= 2 {
        for index in start..=2 {
            matrix[fraction_index][(index - 1) as usize] = 0.0;
        }
    }

    if matrix_index == 1 || matrix_index == 2 {
        let shade = col_float.rem_euclid(1.0);
        matrix[fraction_index][(matrix_index - 1) as usize] *= shade;
    }
}

pub(super) fn update_matrix_with_edge(
    edge_index: usize,
    fraction_index: usize,
    row: i64,
    col: i64,
    geometry: &QuadGeometry,
    matrix: &mut [[f64; 2]; 2],
) {
    match geometry.edge_types[edge_index] {
        EdgeType::Top => {
            update_matrix_with_top_edge(edge_index, fraction_index, row, col, geometry, matrix)
        }
        EdgeType::Bottom => {
            update_matrix_with_bottom_edge(edge_index, fraction_index, row, col, geometry, matrix)
        }
        EdgeType::Left | EdgeType::LeftDiagonal => {
            update_matrix_with_left_edge(edge_index, fraction_index, row, col, geometry, matrix)
        }
        EdgeType::Right | EdgeType::RightDiagonal => {
            update_matrix_with_right_edge(edge_index, fraction_index, row, col, geometry, matrix)
        }
        EdgeType::None => {}
    }
}

pub(super) fn diagonal_blocks_for_slope(
    edge_type: EdgeType,
    slope: f64,
) -> Option<&'static [(f64, &'static str)]> {
    let slope_matches = |expected: f64| (slope - expected).abs() <= 1.0e-9;

    match edge_type {
        EdgeType::LeftDiagonal => {
            if slope_matches(-2.0) {
                Some(RIGHT_DIAGONAL_NEG_2)
            } else if slope_matches(-4.0 / 3.0) {
                Some(RIGHT_DIAGONAL_NEG_4_3)
            } else if slope_matches(-1.0) {
                Some(RIGHT_DIAGONAL_NEG_1)
            } else if slope_matches(-2.0 / 3.0) {
                Some(RIGHT_DIAGONAL_NEG_2_3)
            } else if slope_matches(-1.0 / 3.0) {
                Some(RIGHT_DIAGONAL_NEG_1_3)
            } else if slope_matches(1.0 / 3.0) {
                Some(RIGHT_DIAGONAL_POS_1_3)
            } else if slope_matches(2.0 / 3.0) {
                Some(RIGHT_DIAGONAL_POS_2_3)
            } else if slope_matches(1.0) {
                Some(RIGHT_DIAGONAL_POS_1)
            } else if slope_matches(4.0 / 3.0) {
                Some(RIGHT_DIAGONAL_POS_4_3)
            } else if slope_matches(2.0) {
                Some(RIGHT_DIAGONAL_POS_2)
            } else {
                None
            }
        }
        EdgeType::RightDiagonal => {
            if slope_matches(-2.0) {
                Some(LEFT_DIAGONAL_NEG_2)
            } else if slope_matches(-4.0 / 3.0) {
                Some(LEFT_DIAGONAL_NEG_4_3)
            } else if slope_matches(-1.0) {
                Some(LEFT_DIAGONAL_NEG_1)
            } else if slope_matches(-2.0 / 3.0) {
                Some(LEFT_DIAGONAL_NEG_2_3)
            } else if slope_matches(-1.0 / 3.0) {
                Some(LEFT_DIAGONAL_NEG_1_3)
            } else if slope_matches(1.0 / 3.0) {
                Some(LEFT_DIAGONAL_POS_1_3)
            } else if slope_matches(2.0 / 3.0) {
                Some(LEFT_DIAGONAL_POS_2_3)
            } else if slope_matches(1.0) {
                Some(LEFT_DIAGONAL_POS_1)
            } else if slope_matches(4.0 / 3.0) {
                Some(LEFT_DIAGONAL_POS_4_3)
            } else if slope_matches(2.0) {
                Some(LEFT_DIAGONAL_POS_2)
            } else {
                None
            }
        }
        _ => None,
    }
}
