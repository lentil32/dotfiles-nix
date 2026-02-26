use super::PARTICLE_ZINDEX_OFFSET;
use crate::types::RenderFrame;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[path = "render/cell_draw.rs"]
mod cell_draw;
#[path = "render/geometry.rs"]
mod geometry;
#[path = "render/particles.rs"]
mod particles;

use cell_draw::{
    draw_diagonal_block, draw_horizontally_shifted_sub_block, draw_matrix_character,
    draw_vertically_shifted_sub_block,
};
use geometry::{
    CellIntersections, EdgeType, ensure_clockwise, get_edge_cell_intersection,
    precompute_quad_geometry, update_matrix_with_edge,
};
use particles::draw_particles;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum HighlightRef {
    Normal(u32),
    Inverted(u32),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Glyph {
    Static(&'static str),
}

impl Glyph {
    pub(crate) const BLOCK: Self = Self::Static("█");

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Static(value) => value,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CellOp {
    pub(crate) row: i64,
    pub(crate) col: i64,
    pub(crate) zindex: u32,
    pub(crate) glyph: Glyph,
    pub(crate) highlight: HighlightRef,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ParticleOp {
    pub(crate) cell: CellOp,
    pub(crate) requires_background_probe: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TargetHackOp {
    pub(crate) row: i64,
    pub(crate) col: i64,
    pub(crate) zindex: u32,
    pub(crate) level: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ClearOp {
    pub(crate) max_kept_windows: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RenderPlan {
    pub(crate) clear: Option<ClearOp>,
    pub(crate) cell_ops: Vec<CellOp>,
    pub(crate) particle_ops: Vec<ParticleOp>,
    pub(crate) target_hack: Option<TargetHackOp>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PlannerState {
    pub(crate) bulge_above: bool,
}

impl Default for PlannerState {
    fn default() -> Self {
        Self { bulge_above: false }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Viewport {
    pub(crate) max_row: i64,
    pub(crate) max_col: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct PlannerOutput {
    pub(crate) plan: RenderPlan,
    pub(crate) next_state: PlannerState,
    pub(crate) signature: Option<u64>,
}

pub(crate) struct PlanBuilder {
    viewport: Viewport,
    cell_ops: Vec<CellOp>,
    particle_ops: Vec<ParticleOp>,
}

impl PlanBuilder {
    fn with_capacity(
        viewport: Viewport,
        estimated_cells: usize,
        estimated_particles: usize,
    ) -> Self {
        Self {
            viewport,
            cell_ops: Vec::with_capacity(estimated_cells),
            particle_ops: Vec::with_capacity(estimated_particles),
        }
    }

    fn in_bounds(&self, row: i64, col: i64) -> bool {
        row >= 1 && row <= self.viewport.max_row && col >= 1 && col <= self.viewport.max_col
    }

    pub(super) fn push_cell(
        &mut self,
        row: i64,
        col: i64,
        zindex: u32,
        glyph: Glyph,
        highlight: HighlightRef,
    ) -> bool {
        if !self.in_bounds(row, col) {
            return false;
        }
        self.cell_ops.push(CellOp {
            row,
            col,
            zindex,
            glyph,
            highlight,
        });
        true
    }

    pub(super) fn push_particle(
        &mut self,
        row: i64,
        col: i64,
        zindex: u32,
        glyph: Glyph,
        highlight: HighlightRef,
        requires_background_probe: bool,
    ) -> bool {
        if !self.in_bounds(row, col) {
            return false;
        }
        self.particle_ops.push(ParticleOp {
            cell: CellOp {
                row,
                col,
                zindex,
                glyph,
                highlight,
            },
            requires_background_probe,
        });
        true
    }

    fn finish(self, clear: Option<ClearOp>, target_hack: Option<TargetHackOp>) -> RenderPlan {
        RenderPlan {
            clear,
            cell_ops: self.cell_ops,
            particle_ops: self.particle_ops,
            target_hack,
        }
    }
}

pub(super) struct PlanResources<'a> {
    pub(super) builder: &'a mut PlanBuilder,
    pub(super) windows_zindex: u32,
    pub(super) particle_zindex: u32,
}

fn hash_f64(hasher: &mut DefaultHasher, value: f64) {
    value.to_bits().hash(hasher);
}

pub(crate) fn frame_draw_signature(frame: &RenderFrame) -> Option<u64> {
    if !frame.particles.is_empty() {
        return None;
    }

    let mut hasher = DefaultHasher::new();
    frame.mode.hash(&mut hasher);
    frame.vertical_bar.hash(&mut hasher);
    frame.never_draw_over_target.hash(&mut hasher);
    frame.use_diagonal_blocks.hash(&mut hasher);
    frame.color_levels.hash(&mut hasher);
    frame.windows_zindex.hash(&mut hasher);

    hash_f64(&mut hasher, frame.target.row);
    hash_f64(&mut hasher, frame.target.col);

    for corner in &frame.corners {
        hash_f64(&mut hasher, corner.row);
        hash_f64(&mut hasher, corner.col);
    }

    hash_f64(&mut hasher, frame.max_slope_horizontal);
    hash_f64(&mut hasher, frame.min_slope_vertical);
    hash_f64(&mut hasher, frame.max_angle_difference_diagonal);
    hash_f64(&mut hasher, frame.max_offset_diagonal);
    hash_f64(&mut hasher, frame.min_shade_no_diagonal);
    hash_f64(&mut hasher, frame.min_shade_no_diagonal_vertical_bar);
    hash_f64(&mut hasher, frame.max_shade_no_matrix);
    hash_f64(&mut hasher, frame.gradient_exponent);
    hash_f64(&mut hasher, frame.matrix_pixel_threshold);
    hash_f64(&mut hasher, frame.matrix_pixel_threshold_vertical_bar);
    hash_f64(&mut hasher, frame.matrix_pixel_min_factor);

    if let Some(gradient) = frame.gradient {
        true.hash(&mut hasher);
        hash_f64(&mut hasher, gradient.origin.row);
        hash_f64(&mut hasher, gradient.origin.col);
        hash_f64(&mut hasher, gradient.direction_scaled.row);
        hash_f64(&mut hasher, gradient.direction_scaled.col);
    } else {
        false.hash(&mut hasher);
    }

    Some(hasher.finish())
}

fn compute_gradient_shade(row_center: f64, col_center: f64, frame: &RenderFrame) -> f64 {
    let Some(gradient) = frame.gradient else {
        return 1.0;
    };

    let dy = row_center - gradient.origin.row;
    let dx = col_center - gradient.origin.col;
    let projection =
        (dy * gradient.direction_scaled.row + dx * gradient.direction_scaled.col).clamp(0.0, 1.0);
    (1.0 - projection).powf(frame.gradient_exponent)
}

pub(crate) fn plan_target_hack(frame: &RenderFrame, viewport: Viewport) -> Option<TargetHackOp> {
    if !frame.hide_target_hack || frame.vertical_bar {
        return None;
    }

    let row = frame.target.row.round() as i64;
    let col = frame.target.col.round() as i64;
    if row < 1 || row > viewport.max_row || col < 1 || col > viewport.max_col {
        return None;
    }

    Some(TargetHackOp {
        row,
        col,
        zindex: frame.windows_zindex,
        level: frame.color_levels.max(1),
    })
}

pub(crate) fn render_frame_to_plan(
    frame: &RenderFrame,
    state: PlannerState,
    viewport: Viewport,
) -> PlannerOutput {
    let maybe_signature = frame_draw_signature(frame);
    let corners = ensure_clockwise(&frame.corners);
    let geometry = precompute_quad_geometry(&corners, frame);

    let width = geometry
        .right
        .saturating_sub(geometry.left)
        .saturating_add(1)
        .max(0);
    let height = geometry
        .bottom
        .saturating_sub(geometry.top)
        .saturating_add(1)
        .max(0);
    let estimated_cells = usize::try_from(width.saturating_mul(height)).unwrap_or(0);
    let mut builder = PlanBuilder::with_capacity(viewport, estimated_cells, frame.particles.len());

    let clear = Some(ClearOp {
        max_kept_windows: frame.max_kept_windows,
    });

    if geometry.top > geometry.bottom || geometry.left > geometry.right {
        return PlannerOutput {
            plan: builder.finish(clear, None),
            next_state: state,
            signature: maybe_signature,
        };
    }

    let mut next_state = state;
    next_state.bulge_above = !next_state.bulge_above;
    let bulge_above = next_state.bulge_above;

    let target_row = frame.target.row.round() as i64;
    let target_col = frame.target.col.round() as i64;
    let min_shade_no_diagonal = if frame.vertical_bar {
        frame.min_shade_no_diagonal_vertical_bar
    } else {
        frame.min_shade_no_diagonal
    };

    {
        let mut resources = PlanResources {
            builder: &mut builder,
            windows_zindex: frame.windows_zindex,
            particle_zindex: frame.windows_zindex.saturating_sub(PARTICLE_ZINDEX_OFFSET),
        };

        draw_particles(&mut resources, frame, target_row, target_col);

        for row in geometry.top..=geometry.bottom {
            for col in geometry.left..=geometry.right {
                if frame.never_draw_over_target
                    && !frame.vertical_bar
                    && row == target_row
                    && col == target_col
                {
                    continue;
                }

                let mut intersections = CellIntersections::default();
                let mut single_diagonal = true;
                let mut diagonal_edge_index = None;
                let mut skip_cell = false;

                for edge_index in 0..4 {
                    let intersection =
                        get_edge_cell_intersection(edge_index, row, col, &geometry, false);
                    match geometry.edge_types[edge_index] {
                        EdgeType::LeftDiagonal | EdgeType::RightDiagonal => {
                            let intersection_low =
                                get_edge_cell_intersection(edge_index, row, col, &geometry, true);
                            if intersection_low >= 1.0 {
                                skip_cell = true;
                                break;
                            }

                            if intersection > min_shade_no_diagonal
                                && intersections.diagonal.is_some()
                            {
                                single_diagonal = false;
                            }

                            if intersection > 0.0
                                && intersections
                                    .diagonal
                                    .is_none_or(|current| intersection > current)
                            {
                                intersections.diagonal = Some(intersection);
                                diagonal_edge_index = Some(edge_index);
                            }
                        }
                        edge_type => {
                            if intersection >= 1.0 {
                                skip_cell = true;
                                break;
                            }
                            if intersection > min_shade_no_diagonal {
                                single_diagonal = false;
                            }

                            if intersection > 0.0 {
                                let current = match edge_type {
                                    EdgeType::Top => &mut intersections.top,
                                    EdgeType::Bottom => &mut intersections.bottom,
                                    EdgeType::Left => &mut intersections.left,
                                    EdgeType::Right => &mut intersections.right,
                                    EdgeType::NoEdge => continue,
                                    EdgeType::LeftDiagonal | EdgeType::RightDiagonal => continue,
                                };
                                if current.is_none_or(|existing| intersection > existing) {
                                    *current = Some(intersection);
                                }
                            }
                        }
                    }
                }

                if skip_cell {
                    continue;
                }

                if intersections
                    .diagonal
                    .is_none_or(|diagonal| diagonal < 1.0 - frame.max_shade_no_matrix)
                {
                    let top_intersection = intersections.top.unwrap_or(0.0).max(0.0);
                    let bottom_intersection = intersections.bottom.unwrap_or(0.0).max(0.0);
                    let left_intersection = intersections.left.unwrap_or(0.0).max(0.0);
                    let right_intersection = intersections.right.unwrap_or(0.0).max(0.0);

                    let mut is_vertically_shifted =
                        intersections.top.is_some() || intersections.bottom.is_some();
                    let vertical_shade = 1.0 - top_intersection - bottom_intersection;
                    let mut is_horizontally_shifted =
                        intersections.left.is_some() || intersections.right.is_some();
                    let horizontal_shade = 1.0 - left_intersection - right_intersection;

                    if is_vertically_shifted && is_horizontally_shifted {
                        if vertical_shade < frame.max_shade_no_matrix
                            && horizontal_shade < frame.max_shade_no_matrix
                        {
                            is_vertically_shifted = false;
                            is_horizontally_shifted = false;
                        } else if 2.0 * (1.0 - vertical_shade) > (1.0 - horizontal_shade) {
                            is_horizontally_shifted = false;
                        } else {
                            is_vertically_shifted = false;
                        }
                    }

                    if is_vertically_shifted {
                        let shade = horizontal_shade
                            * compute_gradient_shade(row as f64 + 0.5, col as f64 + 0.5, frame);
                        draw_vertically_shifted_sub_block(
                            &mut resources,
                            frame,
                            bulge_above,
                            row as f64 + top_intersection,
                            row as f64 + 1.0 - bottom_intersection,
                            col,
                            shade,
                        );
                        continue;
                    }

                    if is_horizontally_shifted {
                        if 1.0 - right_intersection <= 1.0 / 8.0
                            && row == target_row
                            && (col == target_col || col == target_col + 1)
                        {
                            continue;
                        }

                        let shade = vertical_shade
                            * compute_gradient_shade(row as f64 + 0.5, col as f64 + 0.5, frame);
                        draw_horizontally_shifted_sub_block(
                            &mut resources,
                            frame,
                            bulge_above,
                            row,
                            col as f64 + left_intersection,
                            col as f64 + 1.0 - right_intersection,
                            shade,
                        );
                        continue;
                    }
                }

                let gradient_shade =
                    compute_gradient_shade(row as f64 + 0.5, col as f64 + 0.5, frame);
                if single_diagonal && frame.use_diagonal_blocks && diagonal_edge_index.is_some() {
                    let has_drawn = draw_diagonal_block(
                        &mut resources,
                        frame,
                        &geometry,
                        diagonal_edge_index.unwrap_or(0),
                        row,
                        col,
                        gradient_shade,
                    );
                    if has_drawn {
                        continue;
                    }
                }

                let mut matrix = [[1.0_f64; 2]; 2];
                for edge_index in 0..4 {
                    for fraction_index in 0..2 {
                        update_matrix_with_edge(
                            edge_index,
                            fraction_index,
                            row,
                            col,
                            &geometry,
                            &mut matrix,
                        );
                    }
                }

                draw_matrix_character(&mut resources, frame, row, col, matrix, gradient_shade);
            }
        }
    }

    PlannerOutput {
        plan: builder.finish(clear, None),
        next_state,
        signature: maybe_signature,
    }
}

#[cfg(test)]
mod tests {
    use super::{PlannerState, Viewport, render_frame_to_plan};
    use crate::types::RenderFrame;
    use crate::types::{Particle, Point};

    fn base_frame() -> RenderFrame {
        RenderFrame {
            mode: "n".to_string(),
            corners: [
                Point {
                    row: 10.0,
                    col: 10.0,
                },
                Point {
                    row: 10.0,
                    col: 11.0,
                },
                Point {
                    row: 11.0,
                    col: 11.0,
                },
                Point {
                    row: 11.0,
                    col: 10.0,
                },
            ],
            target: Point {
                row: 10.0,
                col: 10.0,
            },
            target_corners: [
                Point {
                    row: 10.0,
                    col: 10.0,
                },
                Point {
                    row: 10.0,
                    col: 11.0,
                },
                Point {
                    row: 11.0,
                    col: 11.0,
                },
                Point {
                    row: 11.0,
                    col: 10.0,
                },
            ],
            vertical_bar: false,
            particles: Vec::<Particle>::new(),
            cursor_color: None,
            cursor_color_insert_mode: None,
            normal_bg: None,
            transparent_bg_fallback_color: "#303030".to_string(),
            cterm_cursor_colors: None,
            cterm_bg: None,
            color_at_cursor: None,
            hide_target_hack: false,
            max_kept_windows: 32,
            never_draw_over_target: false,
            use_diagonal_blocks: true,
            max_slope_horizontal: 0.25,
            min_slope_vertical: 4.0,
            max_angle_difference_diagonal: 0.4,
            max_offset_diagonal: 0.4,
            min_shade_no_diagonal: 0.05,
            min_shade_no_diagonal_vertical_bar: 0.05,
            max_shade_no_matrix: 0.95,
            particle_max_lifetime: 1.0,
            particles_over_text: true,
            color_levels: 16,
            gamma: 2.2,
            gradient_exponent: 1.0,
            matrix_pixel_threshold: 0.1,
            matrix_pixel_threshold_vertical_bar: 0.1,
            matrix_pixel_min_factor: 0.4,
            windows_zindex: 200,
            gradient: None,
        }
    }

    #[test]
    fn render_plan_is_deterministic_for_identical_input() {
        let frame = base_frame();
        let viewport = Viewport {
            max_row: 200,
            max_col: 200,
        };
        let state = PlannerState { bulge_above: false };

        let lhs = render_frame_to_plan(&frame, state, viewport);
        let rhs = render_frame_to_plan(&frame, state, viewport);

        assert_eq!(lhs.plan, rhs.plan);
        assert_eq!(lhs.signature, rhs.signature);
    }
}
