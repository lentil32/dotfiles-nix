use crate::core::types::StrokeId;
use std::ops::Deref;
use std::sync::Arc;

pub(crate) const BASE_TIME_INTERVAL: f64 = 1000.0 / 120.0;
pub(crate) const EPSILON: f64 = 1.0e-9;
pub(crate) const DEFAULT_RNG_STATE: u32 = 0xA341_316C;

pub(crate) fn display_metric_row_scale(block_aspect_ratio: f64) -> f64 {
    if block_aspect_ratio.is_finite() {
        block_aspect_ratio.abs().max(EPSILON)
    } else {
        1.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Point {
    pub(crate) row: f64,
    pub(crate) col: f64,
}

impl Point {
    pub(crate) const ZERO: Self = Self { row: 0.0, col: 0.0 };

    pub(crate) fn distance_squared(self, other: Self) -> f64 {
        let dy = self.row - other.row;
        let dx = self.col - other.col;
        dy * dy + dx * dx
    }

    pub(crate) fn display_distance_squared(self, other: Self, block_aspect_ratio: f64) -> f64 {
        let dy = (self.row - other.row) * display_metric_row_scale(block_aspect_ratio);
        let dx = self.col - other.col;
        dy * dy + dx * dx
    }

    pub(crate) fn display_distance(self, other: Self, block_aspect_ratio: f64) -> f64 {
        self.display_distance_squared(other, block_aspect_ratio)
            .sqrt()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct ScreenCell {
    row: i64,
    col: i64,
}

impl ScreenCell {
    pub(crate) fn new(row: i64, col: i64) -> Option<Self> {
        if row < 1 || col < 1 {
            return None;
        }
        Some(Self { row, col })
    }

    pub(crate) fn from_rounded_point(point: Point) -> Option<Self> {
        if !point.row.is_finite() || !point.col.is_finite() {
            return None;
        }

        let rounded_row = point.row.round();
        let rounded_col = point.col.round();
        if rounded_row < 1.0
            || rounded_col < 1.0
            || rounded_row > i64::MAX as f64
            || rounded_col > i64::MAX as f64
        {
            return None;
        }

        Self::new(rounded_row as i64, rounded_col as i64)
    }

    pub(crate) const fn row(self) -> i64 {
        self.row
    }

    pub(crate) const fn col(self) -> i64 {
        self.col
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Particle {
    pub(crate) position: Point,
    pub(crate) velocity: Point,
    pub(crate) lifetime: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RenderStepSample {
    pub(crate) corners: [Point; 4],
    pub(crate) dt_ms: f64,
}

impl RenderStepSample {
    pub(crate) fn new(corners: [Point; 4], dt_ms: f64) -> Self {
        let dt_ms = if dt_ms.is_finite() {
            dt_ms.max(0.0)
        } else {
            0.0
        };
        Self { corners, dt_ms }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StaticRenderConfig {
    pub(crate) cursor_color: Option<String>,
    pub(crate) cursor_color_insert_mode: Option<String>,
    pub(crate) normal_bg: Option<String>,
    pub(crate) transparent_bg_fallback_color: String,
    pub(crate) cterm_cursor_colors: Option<Vec<u16>>,
    pub(crate) cterm_bg: Option<u16>,
    pub(crate) hide_target_hack: bool,
    pub(crate) max_kept_windows: usize,
    pub(crate) never_draw_over_target: bool,
    pub(crate) particle_max_lifetime: f64,
    pub(crate) particle_switch_octant_braille: f64,
    pub(crate) particles_over_text: bool,
    pub(crate) color_levels: u32,
    pub(crate) gamma: f64,
    pub(crate) block_aspect_ratio: f64,
    pub(crate) tail_duration_ms: f64,
    pub(crate) simulation_hz: f64,
    pub(crate) trail_thickness: f64,
    pub(crate) trail_thickness_x: f64,
    pub(crate) spatial_coherence_weight: f64,
    pub(crate) temporal_stability_weight: f64,
    pub(crate) top_k_per_cell: u8,
    pub(crate) windows_zindex: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderFrame {
    pub(crate) mode: String,
    pub(crate) corners: [Point; 4],
    pub(crate) step_samples: Vec<RenderStepSample>,
    pub(crate) planner_idle_steps: u32,
    pub(crate) target: Point,
    pub(crate) target_corners: [Point; 4],
    pub(crate) vertical_bar: bool,
    pub(crate) trail_stroke_id: StrokeId,
    pub(crate) retarget_epoch: u64,
    pub(crate) particles: Vec<Particle>,
    pub(crate) color_at_cursor: Option<String>,
    pub(crate) static_config: Arc<StaticRenderConfig>,
}

impl Deref for RenderFrame {
    type Target = StaticRenderConfig;

    fn deref(&self) -> &Self::Target {
        &self.static_config
    }
}

#[derive(Debug, Clone)]
pub(crate) struct StepInput {
    pub(crate) mode: String,
    pub(crate) time_interval: f64,
    pub(crate) config_time_interval: f64,
    pub(crate) head_response_ms: f64,
    pub(crate) damping_ratio: f64,
    pub(crate) current_corners: [Point; 4],
    pub(crate) trail_origin_corners: [Point; 4],
    pub(crate) target_corners: [Point; 4],
    pub(crate) spring_velocity_corners: [Point; 4],
    pub(crate) trail_elapsed_ms: [f64; 4],
    pub(crate) max_length: f64,
    pub(crate) max_length_insert_mode: f64,
    pub(crate) trail_duration_ms: f64,
    pub(crate) trail_short_duration_ms: f64,
    pub(crate) trail_size: f64,
    pub(crate) trail_min_distance: f64,
    pub(crate) trail_thickness: f64,
    pub(crate) trail_thickness_x: f64,
    pub(crate) particles: Vec<Particle>,
    pub(crate) previous_center: Point,
    pub(crate) particle_damping: f64,
    pub(crate) particles_enabled: bool,
    pub(crate) particle_gravity: f64,
    pub(crate) particle_random_velocity: f64,
    pub(crate) particle_max_num: usize,
    pub(crate) particle_spread: f64,
    pub(crate) particles_per_second: f64,
    pub(crate) particles_per_length: f64,
    pub(crate) particle_max_initial_velocity: f64,
    pub(crate) particle_velocity_from_cursor: f64,
    pub(crate) particle_max_lifetime: f64,
    pub(crate) particle_lifetime_distribution_exponent: f64,
    pub(crate) min_distance_emit_particles: f64,
    pub(crate) vertical_bar: bool,
    pub(crate) horizontal_bar: bool,
    pub(crate) block_aspect_ratio: f64,
    pub(crate) rng_state: u32,
}

#[derive(Debug)]
pub(crate) struct StepOutput {
    pub(crate) current_corners: [Point; 4],
    pub(crate) velocity_corners: [Point; 4],
    pub(crate) spring_velocity_corners: [Point; 4],
    pub(crate) trail_elapsed_ms: [f64; 4],
    pub(crate) particles: Vec<Particle>,
    pub(crate) previous_center: Point,
    pub(crate) index_head: usize,
    pub(crate) index_tail: usize,
    pub(crate) rng_state: u32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Rng32 {
    state: u32,
}

impl Rng32 {
    pub(crate) fn from_seed(seed: u32) -> Self {
        let normalized = if seed == 0 { DEFAULT_RNG_STATE } else { seed };
        Self { state: normalized }
    }

    pub(crate) fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        if x == 0 {
            x = DEFAULT_RNG_STATE;
        }
        self.state = x;
        x
    }

    pub(crate) fn next_unit(&mut self) -> f64 {
        f64::from(self.next_u32()) / f64::from(u32::MAX)
    }

    pub(crate) fn state(self) -> u32 {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::{Point, ScreenCell};

    #[test]
    fn screen_cell_validates_one_indexed_cells() {
        assert_eq!(ScreenCell::new(1, 1), Some(ScreenCell { row: 1, col: 1 }));
        assert_eq!(ScreenCell::new(0, 1), None);
        assert_eq!(ScreenCell::new(1, 0), None);
    }

    #[test]
    fn screen_cell_from_point_rounds_and_rejects_non_finite_values() {
        assert_eq!(
            ScreenCell::from_rounded_point(Point {
                row: 12.6,
                col: 9.4
            }),
            Some(ScreenCell { row: 13, col: 9 })
        );
        assert_eq!(
            ScreenCell::from_rounded_point(Point {
                row: f64::NAN,
                col: 3.0
            }),
            None
        );
        assert_eq!(
            ScreenCell::from_rounded_point(Point {
                row: 3.0,
                col: -1.0
            }),
            None
        );
    }
}
